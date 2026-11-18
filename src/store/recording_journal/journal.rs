use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use chrono::Utc;
use uuid::Uuid;

use super::{
    cipher::JournalCipher,
    frame::{encode_frame, CommitMetadata, Frame, FrameType, JournalBinding, MAX_FRAME_BYTES},
    layout::{acquire_writer_lock, list_segments, new_epoch, open_node_id, segment_path},
    reader::max_observed_sequence,
    JournalError,
};

const COMMIT_KIND: &str = "commit";
const MIN_SEGMENT_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalRecord {
    pub(crate) activity_id: String,
    pub(crate) kind: String,
    pub(crate) parts: Vec<Vec<u8>>,
}

impl JournalRecord {
    pub(crate) fn new(
        activity_id: impl Into<String>,
        kind: impl Into<String>,
        parts: Vec<Vec<u8>>,
    ) -> Self {
        Self {
            activity_id: activity_id.into(),
            kind: kind.into(),
            parts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommitReceipt {
    pub(crate) group_id: String,
    pub(crate) first_sequence: u64,
    pub(crate) last_sequence: u64,
    pub(crate) durable_sequence: u64,
    pub(crate) committed_at: String,
}

pub(crate) struct RecordingJournalWriter {
    root: PathBuf,
    node_id: String,
    epoch: String,
    _lock: File,
    cipher: JournalCipher,
    next_sequence: u64,
    next_segment_index: u32,
    segment_bytes: usize,
    active: Option<ActiveSegment>,
    unhealthy: Option<String>,
}

struct ActiveSegment {
    file: BufWriter<File>,
    bytes: usize,
}

impl RecordingJournalWriter {
    pub(crate) fn open(
        root: impl AsRef<Path>,
        cipher: JournalCipher,
        segment_bytes: usize,
    ) -> Result<Self, JournalError> {
        if segment_bytes < MIN_SEGMENT_BYTES {
            return Err(JournalError::InvalidLayout(format!(
                "journal segment size must be at least {MIN_SEGMENT_BYTES} bytes"
            )));
        }
        let root = root.as_ref();
        fs::create_dir_all(root)?;
        let lock = acquire_writer_lock(root)?;
        let node_id = open_node_id(root)?;
        let segments = list_segments(root, &node_id)?;
        let next_sequence = max_observed_sequence(&node_id, &segments, &cipher)? + 1;
        Ok(Self {
            root: root.to_path_buf(),
            node_id,
            epoch: new_epoch(),
            _lock: lock,
            cipher,
            next_sequence,
            next_segment_index: 1,
            segment_bytes,
            active: None,
            unhealthy: None,
        })
    }

    pub(crate) fn append_and_commit(
        &mut self,
        records: &[JournalRecord],
    ) -> Result<CommitReceipt, JournalError> {
        if let Some(reason) = &self.unhealthy {
            return Err(JournalError::Unhealthy(reason.clone()));
        }
        let (frames, receipt) = self.prepare(records)?;
        let encoded = frames
            .iter()
            .map(|frame| encode_frame(frame, &self.cipher, &self.binding()))
            .collect::<Result<Vec<_>, _>>()?;
        for frame in &encoded {
            if frame.len() > MAX_FRAME_BYTES {
                return Err(JournalError::Corrupt(
                    "journal frame exceeds maximum size".to_string(),
                ));
            }
        }
        if let Err(error) = self.write_prepared(&encoded) {
            self.unhealthy = Some(error.to_string());
            return Err(error);
        }
        self.next_sequence = receipt.durable_sequence + 1;
        Ok(receipt)
    }

    fn prepare(
        &self,
        records: &[JournalRecord],
    ) -> Result<(Vec<Frame>, CommitReceipt), JournalError> {
        if records.is_empty() {
            return Err(JournalError::EmptyRecord);
        }
        let group_id = Uuid::new_v4().to_string();
        let first_sequence = self.next_sequence;
        let mut sequence = first_sequence;
        let mut frames = Vec::new();
        for record in records {
            if record.parts.is_empty() {
                return Err(JournalError::EmptyRecord);
            }
            let part_count = u32::try_from(record.parts.len())
                .map_err(|_| JournalError::Corrupt("too many record parts".to_string()))?;
            for (part_index, part) in record.parts.iter().enumerate() {
                frames.push(Frame {
                    sequence,
                    frame_type: FrameType::Data,
                    activity_id: record.activity_id.clone(),
                    kind: record.kind.clone(),
                    part_index: u32::try_from(part_index)
                        .map_err(|_| JournalError::Corrupt("too many record parts".to_string()))?,
                    part_count,
                    payload: part.clone(),
                });
                sequence = sequence.checked_add(1).ok_or_else(|| {
                    JournalError::Corrupt("journal sequence overflow".to_string())
                })?;
            }
        }
        let last_sequence = sequence - 1;
        let committed_at = Utc::now().to_rfc3339();
        frames.push(Frame {
            sequence,
            frame_type: FrameType::Commit,
            activity_id: String::new(),
            kind: COMMIT_KIND.to_string(),
            part_index: 0,
            part_count: 1,
            payload: serde_json::to_vec(&CommitMetadata {
                group_id: group_id.clone(),
                first_sequence,
                last_sequence,
                committed_at: committed_at.clone(),
            })?,
        });
        Ok((
            frames,
            CommitReceipt {
                group_id,
                first_sequence,
                last_sequence,
                durable_sequence: sequence,
                committed_at,
            },
        ))
    }

    fn write_prepared(&mut self, encoded: &[Vec<u8>]) -> Result<(), JournalError> {
        for frame in encoded {
            let length = u32::try_from(frame.len())
                .map_err(|_| JournalError::Corrupt("journal frame length overflow".to_string()))?;
            let frame_bytes = frame.len() + std::mem::size_of_val(&length);
            if self.active.as_ref().is_some_and(|active| {
                active.bytes > 0 && active.bytes + frame_bytes > self.segment_bytes
            }) {
                self.rotate_active()?;
            }
            if self.active.is_none() {
                self.create_segment()?;
            }
            let active = self
                .active
                .as_mut()
                .expect("active journal segment was created");
            active.file.write_all(&length.to_be_bytes())?;
            active.file.write_all(frame)?;
            active.bytes += frame_bytes;
        }
        self.sync_active()
    }

    fn create_segment(&mut self) -> Result<(), JournalError> {
        loop {
            let path = segment_path(
                &self.root,
                &self.node_id,
                &self.epoch,
                self.next_segment_index,
            );
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => {
                    self.active = Some(ActiveSegment {
                        file: BufWriter::new(file),
                        bytes: 0,
                    });
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    self.next_segment_index =
                        self.next_segment_index.checked_add(1).ok_or_else(|| {
                            JournalError::Corrupt("journal segment index overflow".to_string())
                        })?;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn rotate_active(&mut self) -> Result<(), JournalError> {
        let Some(mut active) = self.active.take() else {
            return Ok(());
        };
        active.file.flush()?;
        active.file.get_ref().sync_all()?;
        self.next_segment_index = self
            .next_segment_index
            .checked_add(1)
            .ok_or_else(|| JournalError::Corrupt("journal segment index overflow".to_string()))?;
        Ok(())
    }

    fn sync_active(&mut self) -> Result<(), JournalError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        active.file.flush()?;
        active.file.get_ref().sync_all()?;
        Ok(())
    }

    fn binding(&self) -> JournalBinding {
        JournalBinding {
            node_id: self.node_id.clone(),
            epoch: self.epoch.clone(),
        }
    }
}

impl Drop for RecordingJournalWriter {
    fn drop(&mut self) {
        if let Some(active) = self.active.as_mut() {
            let _ = active.file.flush();
        }
    }
}
