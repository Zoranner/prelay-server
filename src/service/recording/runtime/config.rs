use std::{path::PathBuf, time::Duration};

use super::RecordingRuntimeError;
use crate::store::recording_journal::GroupCommitConfig;

pub(crate) const DEFAULT_JOURNAL_DIR: &str = "data/recording";
pub(crate) const DEFAULT_SEGMENT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const DEFAULT_CHUNK_BYTES: usize = 1024 * 1024;
pub(crate) const DEFAULT_GROUP_COMMIT_MS: u64 = 2;
pub(crate) const DEFAULT_MATERIALIZE_INTERVAL_MS: u64 = 100;
pub(crate) const DEFAULT_COMMIT_TIMEOUT_MS: u64 = 5000;
pub(crate) const DEFAULT_MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;
pub(crate) const DEFAULT_MAX_PENDING_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub(crate) const DEFAULT_MATERIALIZE_LAG_SLO_SECONDS: u64 = 5;

#[derive(Debug, Clone)]
pub(crate) struct RecordingRuntimeConfig {
    pub(crate) journal_dir: PathBuf,
    pub(crate) segment_bytes: usize,
    pub(crate) chunk_bytes: usize,
    pub(crate) group_commit: GroupCommitConfig,
    pub(crate) materialize_interval: Duration,
    pub(crate) commit_timeout: Duration,
    pub(crate) min_free_bytes: u64,
    pub(crate) max_pending_bytes: u64,
    pub(crate) materialize_lag_slo_seconds: u64,
}

impl RecordingRuntimeConfig {
    pub(crate) fn from_environment() -> Result<Self, RecordingRuntimeError> {
        let journal_dir = std::env::var("RECORDING_JOURNAL_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_JOURNAL_DIR));
        let segment_bytes = parse_usize("RECORDING_JOURNAL_SEGMENT_BYTES", DEFAULT_SEGMENT_BYTES)?;
        let chunk_bytes = parse_usize("RECORDING_JOURNAL_CHUNK_BYTES", DEFAULT_CHUNK_BYTES)?;
        let group_wait_ms =
            parse_u64("RECORDING_JOURNAL_GROUP_COMMIT_MS", DEFAULT_GROUP_COMMIT_MS)?;
        let materialize_interval_ms = parse_u64(
            "RECORDING_MATERIALIZE_INTERVAL_MS",
            DEFAULT_MATERIALIZE_INTERVAL_MS,
        )?;
        let commit_timeout_ms = parse_u64(
            "RECORDING_JOURNAL_COMMIT_TIMEOUT_MS",
            DEFAULT_COMMIT_TIMEOUT_MS,
        )?;
        let min_free_bytes = parse_u64("RECORDING_JOURNAL_MIN_FREE_BYTES", DEFAULT_MIN_FREE_BYTES)?;
        let max_pending_bytes = parse_u64(
            "RECORDING_JOURNAL_MAX_PENDING_BYTES",
            DEFAULT_MAX_PENDING_BYTES,
        )?;
        let materialize_lag_slo_seconds = parse_u64(
            "RECORDING_MATERIALIZE_LAG_SLO_SECONDS",
            DEFAULT_MATERIALIZE_LAG_SLO_SECONDS,
        )?;
        if segment_bytes < 1024 || chunk_bytes == 0 {
            return Err(RecordingRuntimeError::Config(
                "recording segment and chunk sizes must be positive".to_string(),
            ));
        }
        if commit_timeout_ms == 0 {
            return Err(RecordingRuntimeError::Config(
                "recording commit timeout must be positive".to_string(),
            ));
        }
        Ok(Self {
            journal_dir,
            segment_bytes,
            chunk_bytes,
            group_commit: GroupCommitConfig {
                max_wait: Duration::from_millis(group_wait_ms),
                ..GroupCommitConfig::default()
            },
            materialize_interval: Duration::from_millis(materialize_interval_ms.max(1)),
            commit_timeout: Duration::from_millis(commit_timeout_ms),
            min_free_bytes,
            max_pending_bytes,
            materialize_lag_slo_seconds: materialize_lag_slo_seconds.max(1),
        })
    }
}

fn parse_usize(name: &str, default: usize) -> Result<usize, RecordingRuntimeError> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<usize>()
            .map_err(|error| RecordingRuntimeError::Config(format!("{name} is invalid: {error}"))),
        Err(_) => Ok(default),
    }
}

fn parse_u64(name: &str, default: u64) -> Result<u64, RecordingRuntimeError> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|error| RecordingRuntimeError::Config(format!("{name} is invalid: {error}"))),
        Err(_) => Ok(default),
    }
}
