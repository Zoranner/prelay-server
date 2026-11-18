use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use super::{
    journal::{CommitReceipt, JournalRecord, RecordingJournalWriter},
    JournalError,
};

const COMMITTER_THREAD_NAME: &str = "recording-journal-commit";

#[derive(Debug, Clone)]
pub(crate) struct GroupCommitConfig {
    pub(crate) max_wait: Duration,
    pub(crate) max_records: usize,
    pub(crate) max_bytes: usize,
    pub(crate) queue_capacity: usize,
}

impl Default for GroupCommitConfig {
    fn default() -> Self {
        Self {
            max_wait: Duration::from_millis(2),
            max_records: 64,
            max_bytes: 8 * 1024 * 1024,
            queue_capacity: 4096,
        }
    }
}

impl GroupCommitConfig {
    fn validate(&self) -> Result<(), JournalError> {
        if self.max_records == 0 || self.max_bytes == 0 || self.queue_capacity == 0 {
            return Err(JournalError::InvalidLayout(
                "group commit limits must be positive".to_string(),
            ));
        }
        Ok(())
    }
}

pub(crate) struct GroupCommitHandle {
    sender: SyncSender<CommitRequest>,
    healthy: Arc<AtomicBool>,
}

impl Clone for GroupCommitHandle {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            healthy: Arc::clone(&self.healthy),
        }
    }
}

impl GroupCommitHandle {
    pub(crate) fn submit_blocking(
        &self,
        records: Vec<JournalRecord>,
    ) -> Result<CommitWaiter, JournalError> {
        validate_records(&records)?;
        if !self.healthy.load(Ordering::Acquire) {
            return Err(JournalError::Unhealthy(
                "group committer is not running".to_string(),
            ));
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        self.sender
            .send(CommitRequest {
                records,
                response: sender,
            })
            .map_err(|_| JournalError::Unhealthy("group committer has stopped".to_string()))?;
        Ok(CommitWaiter { receiver })
    }

    pub(crate) fn mark_unhealthy(&self, reason: &str) {
        self.healthy.store(false, Ordering::Release);
        tracing::error!(reason = %reason, "recording journal was marked unhealthy");
    }
}

pub(crate) struct CommitWaiter {
    receiver: Receiver<Result<CommitReceipt, JournalError>>,
}

impl CommitWaiter {
    #[cfg(test)]
    pub(crate) fn wait_blocking(self) -> Result<CommitReceipt, JournalError> {
        self.receiver.recv().map_err(|_| {
            JournalError::Unhealthy("group committer stopped before committing".to_string())
        })?
    }

    pub(crate) fn wait_timeout(self, timeout: Duration) -> Result<CommitReceipt, JournalError> {
        self.receiver
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    JournalError::Unhealthy("group commit acknowledgement timed out".to_string())
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    JournalError::Unhealthy("group committer stopped before committing".to_string())
                }
            })?
    }
}

pub(crate) struct GroupCommitter {
    handle: GroupCommitHandle,
    worker: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl GroupCommitter {
    pub(crate) fn start(
        writer: RecordingJournalWriter,
        config: GroupCommitConfig,
    ) -> Result<Self, JournalError> {
        config.validate()?;
        let (sender, receiver) = mpsc::sync_channel(config.queue_capacity);
        let healthy = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_healthy = Arc::clone(&healthy);
        let thread_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name(COMMITTER_THREAD_NAME.to_string())
            .spawn(move || run_committer(writer, config, receiver, thread_healthy, thread_stop))?;
        Ok(Self {
            handle: GroupCommitHandle { sender, healthy },
            worker: Some(worker),
            stop,
        })
    }

    pub(crate) fn handle(&self) -> GroupCommitHandle {
        self.handle.clone()
    }

    pub(crate) fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        drop(self.handle);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct CommitRequest {
    records: Vec<JournalRecord>,
    response: SyncSender<Result<CommitReceipt, JournalError>>,
}

struct PreparedRequest {
    records: Vec<JournalRecord>,
    part_count: u64,
    estimated_bytes: usize,
    response: SyncSender<Result<CommitReceipt, JournalError>>,
}

fn run_committer(
    mut writer: RecordingJournalWriter,
    config: GroupCommitConfig,
    receiver: Receiver<CommitRequest>,
    healthy: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    let mut pending = None;
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        let first = match pending.take() {
            Some(request) => request,
            None => match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(request) => request,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            },
        };
        let mut group = vec![prepare(first)];
        let started_at = Instant::now();
        pending = collect_group(&receiver, &config, started_at, &mut group);
        let records = group
            .iter()
            .flat_map(|request| request.records.iter().cloned())
            .collect::<Vec<_>>();
        let result = writer.append_and_commit(&records);
        match result {
            Ok(receipt) => publish_success(group, receipt),
            Err(error) => {
                publish_failure(group, &error);
                if is_fatal(&error) {
                    healthy.store(false, Ordering::Release);
                    break;
                }
            }
        }
    }
    healthy.store(false, Ordering::Release);
}

fn collect_group(
    receiver: &Receiver<CommitRequest>,
    config: &GroupCommitConfig,
    started_at: Instant,
    group: &mut Vec<PreparedRequest>,
) -> Option<CommitRequest> {
    let mut record_count: usize = group.iter().map(|request| request.records.len()).sum();
    let mut byte_count = group
        .iter()
        .map(|request| request.estimated_bytes)
        .sum::<usize>();
    while record_count < config.max_records {
        let Some(remaining) = config.max_wait.checked_sub(started_at.elapsed()) else {
            break;
        };
        let request = match receiver.recv_timeout(remaining) {
            Ok(request) => prepare(request),
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        };
        let next_records = record_count + request.records.len();
        let next_bytes = byte_count.saturating_add(request.estimated_bytes);
        if next_records > config.max_records || next_bytes > config.max_bytes {
            return Some(unprepare(request));
        }
        record_count = next_records;
        byte_count = next_bytes;
        group.push(request);
    }
    None
}

fn prepare(request: CommitRequest) -> PreparedRequest {
    let part_count = request
        .records
        .iter()
        .map(|record| record.parts.len() as u64)
        .sum();
    let estimated_bytes = request
        .records
        .iter()
        .flat_map(|record| record.parts.iter())
        .map(|part| part.len().saturating_mul(2).saturating_add(256))
        .sum();
    PreparedRequest {
        records: request.records,
        part_count,
        estimated_bytes,
        response: request.response,
    }
}

fn unprepare(request: PreparedRequest) -> CommitRequest {
    CommitRequest {
        records: request.records,
        response: request.response,
    }
}

fn publish_success(group: Vec<PreparedRequest>, receipt: CommitReceipt) {
    let mut next_sequence = receipt.first_sequence;
    for request in group {
        let last_sequence = next_sequence + request.part_count - 1;
        let _ = request.response.send(Ok(CommitReceipt {
            group_id: receipt.group_id.clone(),
            first_sequence: next_sequence,
            last_sequence,
            durable_sequence: receipt.durable_sequence,
            committed_at: receipt.committed_at.clone(),
        }));
        next_sequence = last_sequence + 1;
    }
}

fn publish_failure(group: Vec<PreparedRequest>, error: &JournalError) {
    for request in group {
        let _ = request.response.send(Err(waiter_error(error)));
    }
}

fn validate_records(records: &[JournalRecord]) -> Result<(), JournalError> {
    if records.is_empty() || records.iter().any(|record| record.parts.is_empty()) {
        return Err(JournalError::EmptyRecord);
    }
    Ok(())
}

fn is_fatal(error: &JournalError) -> bool {
    matches!(
        error,
        JournalError::Io(_)
            | JournalError::Json(_)
            | JournalError::Crypto(_)
            | JournalError::InvalidLayout(_)
            | JournalError::Locked(_)
            | JournalError::Unhealthy(_)
    )
}

fn waiter_error(error: &JournalError) -> JournalError {
    match error {
        JournalError::EmptyRecord => JournalError::EmptyRecord,
        JournalError::Corrupt(message) => JournalError::Corrupt(message.clone()),
        _ => JournalError::Unhealthy(error.to_string()),
    }
}
