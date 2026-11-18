use std::{
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecordingHealthStatus {
    Ready,
    Degraded,
    Blocked,
}

impl RecordingHealthStatus {
    pub(super) fn as_u8(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Degraded => 1,
            Self::Blocked => 2,
        }
    }

    pub(super) fn from_u8(value: u8) -> Self {
        match value {
            2 => Self::Blocked,
            1 => Self::Degraded,
            _ => Self::Ready,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecordingHealthSnapshot {
    pub(crate) status: RecordingHealthStatus,
    pub(crate) reason: Option<String>,
    pub(crate) last_applied_sequence: u64,
    pub(crate) durable_sequence: u64,
    pub(crate) committed_data_sequence: u64,
    pub(crate) materialize_lag_seconds: u64,
    pub(crate) pending_response_states: usize,
    pub(crate) materialize_lag_slo_seconds: u64,
}

pub(super) struct RuntimeHealth {
    pub(super) status: AtomicU8,
    pub(super) reason: RwLock<Option<String>>,
    pub(super) last_applied_sequence: Arc<AtomicU64>,
    pub(super) durable_sequence: Arc<AtomicU64>,
    pub(super) committed_data_sequence: AtomicU64,
    pub(super) journal_bytes: AtomicU64,
    pub(super) pending_since: Mutex<Option<Instant>>,
}

impl RuntimeHealth {
    pub(super) fn new() -> Self {
        Self {
            status: AtomicU8::new(RecordingHealthStatus::Ready.as_u8()),
            reason: RwLock::new(None),
            last_applied_sequence: Arc::new(AtomicU64::new(0)),
            durable_sequence: Arc::new(AtomicU64::new(0)),
            committed_data_sequence: AtomicU64::new(0),
            journal_bytes: AtomicU64::new(0),
            pending_since: Mutex::new(None),
        }
    }

    pub(super) fn set_ready(&self) {
        if self.status.load(Ordering::Acquire) != RecordingHealthStatus::Blocked.as_u8() {
            self.status
                .store(RecordingHealthStatus::Ready.as_u8(), Ordering::Release);
            if let Ok(mut reason) = self.reason.write() {
                *reason = None;
            }
        }
    }

    pub(super) fn set_degraded(&self, reason: impl Into<String>) {
        if self.status.load(Ordering::Acquire) == RecordingHealthStatus::Blocked.as_u8() {
            return;
        }
        self.status
            .store(RecordingHealthStatus::Degraded.as_u8(), Ordering::Release);
        if let Ok(mut current) = self.reason.write() {
            *current = Some(reason.into());
        }
    }

    pub(super) fn set_blocked(&self, reason: impl Into<String>) {
        self.status
            .store(RecordingHealthStatus::Blocked.as_u8(), Ordering::Release);
        if let Ok(mut current) = self.reason.write() {
            *current = Some(reason.into());
        }
    }

    pub(super) fn record_commit(&self, last_sequence: u64) {
        self.committed_data_sequence
            .fetch_max(last_sequence, Ordering::AcqRel);
        if let Ok(mut pending_since) = self.pending_since.lock() {
            pending_since.get_or_insert_with(Instant::now);
        }
    }

    pub(super) fn record_materialized(&self, last_applied: u64, slo: Duration) {
        self.last_applied_sequence
            .store(last_applied, Ordering::Release);
        let committed = self.committed_data_sequence.load(Ordering::Acquire);
        if last_applied >= committed {
            if let Ok(mut pending_since) = self.pending_since.lock() {
                *pending_since = None;
            }
            self.set_ready();
            return;
        }
        let lag = self
            .pending_since
            .lock()
            .ok()
            .and_then(|pending_since| *pending_since)
            .map(|started_at| started_at.elapsed())
            .unwrap_or_default();
        if lag > slo {
            self.set_degraded(format!(
                "recording materialization lag is {} seconds, above the SLO of {} seconds",
                lag.as_secs(),
                slo.as_secs()
            ));
        }
    }

    pub(super) fn materialize_lag_seconds(&self) -> u64 {
        self.pending_since
            .lock()
            .ok()
            .and_then(|pending_since| *pending_since)
            .map(|started_at| started_at.elapsed().as_secs())
            .unwrap_or_default()
    }
}
