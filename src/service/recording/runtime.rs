use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use tokio::{sync::oneshot, task::JoinHandle};

use crate::store::{
    recording_journal::{
        ActivityMaterializeSink, GroupCommitHandle, GroupCommitter, JournalCipher, JournalRecord,
        Materializer, PendingResponseStates, RecordEnvelope, RecordingJournalWriter,
    },
    rollout::RolloutStore,
    storage::{MasterKey, ResponseState, Storage},
};

const RESPONSE_STATE_CACHE_MAX_ENTRIES: usize = 10_000;
const MATERIALIZER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const JOURNAL_BYTES_REFRESH: Duration = Duration::from_secs(1);

mod config;
mod error;
mod health;
mod pending;
mod retry;

pub(crate) use config::RecordingRuntimeConfig;
pub(crate) use error::RecordingRuntimeError;
use health::RuntimeHealth;
pub(crate) use health::{RecordingHealthSnapshot, RecordingHealthStatus};
use pending::pending_response_state;
use retry::retry_delay;

#[derive(Clone)]
pub struct RecordingRuntime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    commit: GroupCommitHandle,
    chunk_bytes: usize,
    commit_timeout: Duration,
    journal_dir: PathBuf,
    min_free_bytes: u64,
    max_pending_bytes: u64,
    materialize_lag_slo_seconds: u64,
    response_state_cache_max_entries: usize,
    health: Arc<RuntimeHealth>,
    pending_response_states: PendingResponseStates,
    materializer_start: Mutex<Option<oneshot::Sender<()>>>,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    materializer_task: Mutex<Option<JoinHandle<()>>>,
    committer: Mutex<Option<GroupCommitter>>,
}

impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if let Ok(mut shutdown) = self.shutdown.lock() {
            if let Some(sender) = shutdown.take() {
                let _ = sender.send(());
            }
        }
        if let Ok(mut committer) = self.committer.lock() {
            if let Some(committer) = committer.take() {
                let _ = std::thread::Builder::new()
                    .name("recording-committer-shutdown".to_string())
                    .spawn(move || committer.shutdown());
            }
        }
    }
}

impl RecordingRuntime {
    pub(crate) async fn start(
        config: RecordingRuntimeConfig,
        master_key: &MasterKey,
        storage: Storage,
        rollouts: RolloutStore,
    ) -> Result<Self, RecordingRuntimeError> {
        let cipher = JournalCipher::new(master_key);
        std::fs::create_dir_all(&config.journal_dir).map_err(|error| {
            RecordingRuntimeError::Materializer(format!(
                "unable to create recording journal directory {}: {error}",
                config.journal_dir.display()
            ))
        })?;
        let writer = RecordingJournalWriter::open(
            &config.journal_dir,
            cipher.clone(),
            config.segment_bytes,
        )?;
        let pending_response_states = PendingResponseStates::default();
        let sink = Arc::new(ActivityMaterializeSink::new(
            storage,
            rollouts,
            pending_response_states.clone(),
        ));
        let mut materializer = Materializer::open(&config.journal_dir, Arc::clone(&sink))
            .map_err(|error| RecordingRuntimeError::Materializer(error.to_string()))?;
        let replay = materializer
            .replay_all(&cipher)
            .await
            .map_err(|error| RecordingRuntimeError::Materializer(error.to_string()))?;
        if replay.applied_records > 0 {
            tracing::info!(
                applied_records = replay.applied_records,
                skipped_records = replay.skipped_records,
                batches = replay.batches,
                "replayed committed recording journal at startup"
            );
        }
        let health = Arc::new(RuntimeHealth::new());
        health.journal_bytes.store(
            directory_bytes(&config.journal_dir).unwrap_or_default(),
            Ordering::Release,
        );
        health
            .last_applied_sequence
            .store(materializer.last_applied_sequence(), Ordering::Release);
        let committer = GroupCommitter::start(writer, config.group_commit)?;
        let commit = committer.handle();

        let (start_tx, mut start_rx) = oneshot::channel();
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let interval = config.materialize_interval;
        let task_health = Arc::clone(&health);
        let task_journal_dir = config.journal_dir.clone();
        let materialize_lag_slo = Duration::from_secs(config.materialize_lag_slo_seconds);
        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            let mut last_bytes_refresh = Instant::now();
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tokio::select! {
                _ = &mut start_rx => {}
                _ = &mut shutdown_rx => return,
            }
            let mut consecutive_failures = 0_u32;
            loop {
                let mut pending_retry_delay = None;
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    _ = ticker.tick() => {
                        match materializer.replay_all(&cipher).await {
                            Ok(_) => {
                                consecutive_failures = 0;
                                task_health.record_materialized(
                                    materializer.last_applied_sequence(),
                                    materialize_lag_slo,
                                );
                                if last_bytes_refresh.elapsed() >= JOURNAL_BYTES_REFRESH {
                                    let path = task_journal_dir.clone();
                                    let bytes = match tokio::task::spawn_blocking(move || {
                                        directory_bytes(&path)
                                    })
                                    .await
                                    {
                                        Ok(Ok(bytes)) => bytes,
                                        _ => 0,
                                    };
                                    task_health
                                        .journal_bytes
                                        .store(bytes, Ordering::Release);
                                    last_bytes_refresh = Instant::now();
                                }
                            }
                            Err(error) => {
                                if error.is_blocking() {
                                    task_health.set_blocked(
                                        "recording journal is corrupt or unavailable",
                                    );
                                } else {
                                    task_health.set_degraded(
                                        "recording materialization is delayed or unavailable",
                                    );
                                }
                                tracing::error!(
                                    error = %error,
                                    "recording materializer failed; its committed journal is retained"
                                );
                                consecutive_failures = consecutive_failures.saturating_add(1);
                                pending_retry_delay = Some(retry_delay(consecutive_failures));
                            }
                        }
                    }
                }
                // 失败后按退避重试：物化卡住时不再每个 tick 刷一条同样的错误，
                // 同时保留 shutdown 的响应性。
                if let Some(delay) = pending_retry_delay {
                    tokio::select! {
                        _ = &mut shutdown_rx => break,
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
            if let Err(error) = materializer.replay_all(&cipher).await {
                tracing::error!(
                    error = %error,
                    "recording materializer final drain failed"
                );
            } else {
                task_health
                    .record_materialized(materializer.last_applied_sequence(), materialize_lag_slo);
                task_health.journal_bytes.store(
                    directory_bytes(&task_journal_dir).unwrap_or_default(),
                    Ordering::Release,
                );
            }
        });

        Ok(Self {
            inner: Arc::new(RuntimeInner {
                commit,
                chunk_bytes: config.chunk_bytes,
                commit_timeout: config.commit_timeout,
                journal_dir: config.journal_dir,
                min_free_bytes: config.min_free_bytes,
                max_pending_bytes: config.max_pending_bytes,
                materialize_lag_slo_seconds: config.materialize_lag_slo_seconds,
                response_state_cache_max_entries: RESPONSE_STATE_CACHE_MAX_ENTRIES,
                health,
                pending_response_states,
                materializer_start: Mutex::new(Some(start_tx)),
                shutdown: Mutex::new(Some(shutdown_tx)),
                materializer_task: Mutex::new(Some(task)),
                committer: Mutex::new(Some(committer)),
            }),
        })
    }

    pub(crate) async fn commit_envelope(
        &self,
        envelope: RecordEnvelope,
    ) -> Result<(), RecordingRuntimeError> {
        self.preflight()?;
        let pending_state = pending_response_state(&envelope);
        if let Some(state) = pending_state.as_ref() {
            if self.inner.pending_response_states.len()
                >= self.inner.response_state_cache_max_entries
                && self
                    .inner
                    .pending_response_states
                    .get(&state.identity_id, &state.response_id)
                    .is_none()
            {
                self.inner
                    .health
                    .set_degraded("recording response state cache is full");
                return Err(RecordingRuntimeError::Capacity(
                    "recording response state cache is full".to_string(),
                ));
            }
        }
        let record = JournalRecord::from_envelope(envelope, self.inner.chunk_bytes)?;
        if let Some(state) = pending_state.as_ref() {
            self.inner.pending_response_states.insert(state.clone());
        }
        let commit = self.inner.commit.clone();
        let timeout = self.inner.commit_timeout;
        let receipt = tokio::task::spawn_blocking(move || {
            commit.submit_blocking(vec![record])?.wait_timeout(timeout)
        })
        .await;
        let receipt = match receipt {
            Ok(Ok(receipt)) => receipt,
            Ok(Err(error)) => {
                if let Some(state) = pending_state.as_ref() {
                    self.inner
                        .pending_response_states
                        .remove(&state.identity_id, &state.response_id);
                }
                tracing::error!(error = %error, "recording journal commit failed");
                self.inner.commit.mark_unhealthy(&error.to_string());
                self.inner
                    .health
                    .set_blocked("recording journal commit failed");
                return Err(RecordingRuntimeError::from(error));
            }
            Err(error) => {
                if let Some(state) = pending_state.as_ref() {
                    self.inner
                        .pending_response_states
                        .remove(&state.identity_id, &state.response_id);
                }
                self.inner.health.set_blocked("recording worker failed");
                return Err(RecordingRuntimeError::Worker(error.to_string()));
            }
        };
        self.inner
            .health
            .durable_sequence
            .store(receipt.durable_sequence, Ordering::Release);
        self.inner.health.record_commit(receipt.last_sequence);
        Ok(())
    }
    pub(crate) fn pending_response_state(
        &self,
        identity_id: &str,
        response_id: &str,
    ) -> Option<ResponseState> {
        self.inner
            .pending_response_states
            .get(identity_id, response_id)
    }

    pub(crate) fn health_snapshot(&self) -> RecordingHealthSnapshot {
        let status =
            RecordingHealthStatus::from_u8(self.inner.health.status.load(Ordering::Acquire));
        let reason = self
            .inner
            .health
            .reason
            .read()
            .ok()
            .and_then(|reason| reason.clone());
        RecordingHealthSnapshot {
            status,
            reason,
            last_applied_sequence: self
                .inner
                .health
                .last_applied_sequence
                .load(Ordering::Acquire),
            durable_sequence: self.inner.health.durable_sequence.load(Ordering::Acquire),
            committed_data_sequence: self
                .inner
                .health
                .committed_data_sequence
                .load(Ordering::Acquire),
            materialize_lag_seconds: self.inner.health.materialize_lag_seconds(),
            pending_response_states: self.inner.pending_response_states.len(),
            materialize_lag_slo_seconds: self.inner.materialize_lag_slo_seconds,
        }
    }

    pub(crate) fn sequence_handles(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>) {
        (
            Arc::clone(&self.inner.health.durable_sequence),
            Arc::clone(&self.inner.health.last_applied_sequence),
        )
    }

    pub(crate) fn start_materialization(&self) {
        if let Ok(mut start) = self.inner.materializer_start.lock() {
            if let Some(sender) = start.take() {
                let _ = sender.send(());
            }
        }
    }

    pub(crate) async fn shutdown(&self) {
        if let Ok(mut shutdown) = self.inner.shutdown.lock() {
            if let Some(sender) = shutdown.take() {
                let _ = sender.send(());
            }
        }
        let task = self
            .inner
            .materializer_task
            .lock()
            .ok()
            .and_then(|mut task| task.take());
        if let Some(task) = task {
            if tokio::time::timeout(MATERIALIZER_SHUTDOWN_TIMEOUT, task)
                .await
                .is_err()
            {
                tracing::warn!(
                    timeout_seconds = MATERIALIZER_SHUTDOWN_TIMEOUT.as_secs(),
                    "recording materializer did not drain before shutdown timeout"
                );
            }
        }
        let committer = self
            .inner
            .committer
            .lock()
            .ok()
            .and_then(|mut committer| committer.take());
        if let Some(committer) = committer {
            let _ = tokio::task::spawn_blocking(move || committer.shutdown()).await;
        }
    }

    fn preflight(&self) -> Result<(), RecordingRuntimeError> {
        if self.inner.health.status.load(Ordering::Acquire)
            == RecordingHealthStatus::Blocked.as_u8()
        {
            return Err(RecordingRuntimeError::Capacity(
                "recording journal is blocked".to_string(),
            ));
        }
        if self.inner.min_free_bytes > 0 {
            let free = fs2::available_space(&self.inner.journal_dir).map_err(|error| {
                RecordingRuntimeError::Capacity(format!(
                    "unable to inspect recording journal free space: {error}"
                ))
            })?;
            if free < self.inner.min_free_bytes {
                self.inner
                    .health
                    .set_blocked("recording journal free space is below the minimum");
                return Err(RecordingRuntimeError::Capacity(format!(
                    "recording journal free space is {free} bytes, below the minimum {} bytes",
                    self.inner.min_free_bytes
                )));
            }
        }
        if self.inner.max_pending_bytes > 0 {
            let bytes = self.inner.health.journal_bytes.load(Ordering::Acquire);
            if bytes > self.inner.max_pending_bytes {
                self.inner
                    .health
                    .set_blocked("recording journal pending bytes exceed the limit");
                return Err(RecordingRuntimeError::Capacity(format!(
                    "recording journal has {bytes} bytes, above the pending limit {} bytes",
                    self.inner.max_pending_bytes
                )));
            }
        }
        Ok(())
    }
}

fn directory_bytes(path: &Path) -> std::io::Result<u64> {
    let mut total: u64 = 0;
    if !path.exists() {
        return Ok(0);
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        } else if metadata.is_dir() {
            total = total.saturating_add(directory_bytes(&entry.path())?);
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests;
