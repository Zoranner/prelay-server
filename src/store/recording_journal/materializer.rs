use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use futures::future::BoxFuture;

use super::{
    checkpoint::JournalCheckpoint,
    cipher::JournalCipher,
    frame::{CommittedBatch, Frame, FrameType},
    journal::JournalRecord,
    layout::{list_segments, open_node_id},
    reader::RecordingJournalReader,
    record::RecordEnvelope,
    JournalError,
};

/// 单轮物化最多读取的已编码字节数。
///
/// 物化失败时会一直重试同一批记录；如果不限制单轮读取量，每轮都会把整个积压重新读入
/// 并解密一遍，积压越大越慢，最终拖垮进程。
const DEFAULT_REPLAY_BUDGET_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum MaterializeError {
    Journal(JournalError),
    InvalidBatch(String),
    Sink(String),
}

impl fmt::Display for MaterializeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Journal(error) => error.fmt(formatter),
            Self::InvalidBatch(message) => {
                write!(formatter, "invalid materialize batch: {message}")
            }
            Self::Sink(message) => write!(formatter, "materialize sink failed: {message}"),
        }
    }
}

impl std::error::Error for MaterializeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Journal(error) => Some(error),
            Self::InvalidBatch(_) | Self::Sink(_) => None,
        }
    }
}

impl From<JournalError> for MaterializeError {
    fn from(error: JournalError) -> Self {
        Self::Journal(error)
    }
}

impl MaterializeError {
    pub(crate) fn is_blocking(&self) -> bool {
        matches!(self, Self::Journal(_) | Self::InvalidBatch(_))
    }
}

pub(crate) trait MaterializeSink: Send + Sync {
    fn apply(&self, record: RecordEnvelope) -> BoxFuture<'static, Result<(), MaterializeError>>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MaterializeSummary {
    pub(crate) batches: usize,
    pub(crate) applied_records: usize,
    pub(crate) skipped_records: usize,
}

pub(crate) struct Materializer<S: MaterializeSink> {
    root: PathBuf,
    node_id: String,
    sink: Arc<S>,
    checkpoint: Option<JournalCheckpoint>,
    replay_budget_bytes: usize,
}

impl<S: MaterializeSink> Materializer<S> {
    pub(crate) fn open(root: impl AsRef<Path>, sink: Arc<S>) -> Result<Self, MaterializeError> {
        let root = root.as_ref();
        let node_id = open_node_id(root)?;
        let checkpoint = JournalCheckpoint::load(root, &node_id)?;
        Ok(Self {
            root: root.to_path_buf(),
            node_id,
            sink,
            checkpoint,
            replay_budget_bytes: DEFAULT_REPLAY_BUDGET_BYTES,
        })
    }

    #[cfg(test)]
    pub(crate) fn set_replay_budget(&mut self, bytes: usize) {
        self.replay_budget_bytes = bytes.max(1);
    }

    pub(crate) async fn replay_all(
        &mut self,
        cipher: &JournalCipher,
    ) -> Result<MaterializeSummary, MaterializeError> {
        let batches = RecordingJournalReader::read_committed_after_bounded(
            &self.root,
            cipher,
            self.checkpoint.as_ref(),
            self.replay_budget_bytes,
        )?;
        match self.apply_batches(&batches).await {
            Ok(summary) => {
                self.cleanup_applied_segments()?;
                Ok(summary)
            }
            Err(error) => {
                // 本轮已经推进的批次必须立刻回收，否则物化失败期间磁盘只增不减。
                if let Err(cleanup_error) = self.cleanup_applied_segments() {
                    tracing::warn!(
                        error = %cleanup_error,
                        "failed to reclaim applied journal segments after a materialize failure"
                    );
                }
                Err(error)
            }
        }
    }

    pub(crate) async fn apply_batches(
        &mut self,
        batches: &[CommittedBatch],
    ) -> Result<MaterializeSummary, MaterializeError> {
        let mut summary = MaterializeSummary::default();
        for batch in batches {
            let (applied, skipped) = self.apply_batch(batch).await?;
            summary.batches += 1;
            summary.applied_records += applied;
            summary.skipped_records += skipped;
            if applied > 0 {
                self.save_checkpoint(batch)?;
            }
        }
        Ok(summary)
    }

    #[cfg(test)]
    pub(crate) fn checkpoint(&self) -> Option<&JournalCheckpoint> {
        self.checkpoint.as_ref()
    }

    pub(crate) fn last_applied_sequence(&self) -> u64 {
        self.checkpoint
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.last_applied_sequence)
    }

    async fn apply_batch(
        &mut self,
        batch: &CommittedBatch,
    ) -> Result<(usize, usize), MaterializeError> {
        let records = logical_records(batch)?;
        let mut applied = 0;
        let mut skipped = 0;
        let mut last_applied_sequence = self.last_applied_sequence();
        for record in records {
            if record.last_sequence <= last_applied_sequence {
                skipped += 1;
                continue;
            }
            if record.first_sequence <= last_applied_sequence {
                return Err(MaterializeError::InvalidBatch(format!(
                    "checkpoint splits logical record at sequence {}",
                    record.first_sequence
                )));
            }
            let record_kind = record.kind.clone();
            let record_activity = record.activity_id.clone();
            let record_sequence = record.first_sequence;
            let journal_record = JournalRecord::new(record.activity_id, record.kind, record.parts);
            let envelope = journal_record.decode_envelope()?;
            self.sink
                .apply(envelope)
                .await
                .map_err(|error| match error {
                    MaterializeError::Sink(message) => MaterializeError::Sink(format!(
                        "{record_kind} {record_activity} at sequence {record_sequence}: {message}"
                    )),
                    other => other,
                })?;
            last_applied_sequence = record.last_sequence;
            applied += 1;
        }
        if applied > 0 {
            self.checkpoint = Some(JournalCheckpoint {
                node_id: self.node_id.clone(),
                epoch: batch.position.epoch.clone(),
                segment: batch.position.segment,
                offset: batch.position.offset,
                last_applied_sequence,
                updated_at: Utc::now().to_rfc3339(),
            });
        }
        Ok((applied, skipped))
    }

    fn save_checkpoint(&mut self, batch: &CommittedBatch) -> Result<(), MaterializeError> {
        let Some(checkpoint) = self.checkpoint.as_ref() else {
            return Ok(());
        };
        let checkpoint = JournalCheckpoint {
            node_id: checkpoint.node_id.clone(),
            epoch: batch.position.epoch.clone(),
            segment: batch.position.segment,
            offset: batch.position.offset,
            last_applied_sequence: checkpoint.last_applied_sequence,
            updated_at: Utc::now().to_rfc3339(),
        };
        checkpoint.save(&self.root)?;
        self.checkpoint = Some(checkpoint);
        Ok(())
    }

    fn cleanup_applied_segments(&self) -> Result<(), MaterializeError> {
        let Some(checkpoint) = self.checkpoint.as_ref() else {
            return Ok(());
        };
        let segments = list_segments(&self.root, &self.node_id)?;
        for segment in segments {
            if (segment.epoch.as_str(), segment.index)
                >= (checkpoint.epoch.as_str(), checkpoint.segment)
            {
                continue;
            }
            match fs::remove_file(&segment.path) {
                Ok(()) => tracing::debug!(
                    segment = %segment.path.display(),
                    "removed a fully applied recording journal segment"
                ),
                Err(error) => tracing::warn!(
                    segment = %segment.path.display(),
                    error = %error,
                    "failed to remove an applied recording journal segment"
                ),
            }
        }
        Ok(())
    }
}

struct LogicalRecord {
    activity_id: String,
    kind: String,
    parts: Vec<Vec<u8>>,
    first_sequence: u64,
    last_sequence: u64,
}

fn logical_records(batch: &CommittedBatch) -> Result<Vec<LogicalRecord>, MaterializeError> {
    let mut records = Vec::new();
    let mut frames = batch.records.iter().peekable();
    while let Some(first) = frames.next() {
        if first.frame_type != FrameType::Data || first.part_index != 0 {
            return Err(MaterializeError::InvalidBatch(
                "commit batch starts in the middle of a logical record".to_string(),
            ));
        }
        let part_count = first.part_count as usize;
        if part_count == 0 {
            return Err(MaterializeError::InvalidBatch(
                "logical record has no parts".to_string(),
            ));
        }
        let mut parts = Vec::with_capacity(part_count);
        parts.push(first.payload.clone());
        let mut last_sequence = first.sequence;
        for expected_index in 1..part_count {
            let Some(frame) = frames.next() else {
                return Err(MaterializeError::InvalidBatch(format!(
                    "logical record {}:{} is missing part {expected_index}",
                    first.activity_id, first.kind
                )));
            };
            validate_part(first, frame, expected_index)?;
            if frame.sequence != first.sequence + expected_index as u64 {
                return Err(MaterializeError::InvalidBatch(
                    "logical record parts are not contiguous".to_string(),
                ));
            }
            parts.push(frame.payload.clone());
            last_sequence = frame.sequence;
        }
        records.push(LogicalRecord {
            activity_id: first.activity_id.clone(),
            kind: first.kind.clone(),
            parts,
            first_sequence: first.sequence,
            last_sequence,
        });
    }
    Ok(records)
}

fn validate_part(
    first: &Frame,
    frame: &Frame,
    expected_index: usize,
) -> Result<(), MaterializeError> {
    if frame.frame_type != FrameType::Data
        || frame.activity_id != first.activity_id
        || frame.kind != first.kind
        || frame.part_count != first.part_count
        || frame.part_index != expected_index as u32
    {
        return Err(MaterializeError::InvalidBatch(format!(
            "logical record {}:{} has inconsistent parts",
            first.activity_id, first.kind
        )));
    }
    Ok(())
}
