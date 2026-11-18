use std::{collections::HashMap, fs, path::Path};

use super::{
    checkpoint::JournalCheckpoint,
    cipher::JournalCipher,
    frame::{
        decode_frame, BatchPosition, CommitMetadata, CommittedBatch, Frame, FrameType,
        JournalBinding, MAX_FRAME_BYTES,
    },
    layout::{list_segments, read_node_id, JournalSegment},
    JournalError,
};

pub(crate) struct RecordingJournalReader;

impl RecordingJournalReader {
    #[cfg(test)]
    pub(crate) fn read_committed(
        root: impl AsRef<Path>,
        cipher: &JournalCipher,
    ) -> Result<Vec<CommittedBatch>, JournalError> {
        Self::read_committed_after(root, cipher, None)
    }

    pub(crate) fn read_committed_after(
        root: impl AsRef<Path>,
        cipher: &JournalCipher,
        checkpoint: Option<&JournalCheckpoint>,
    ) -> Result<Vec<CommittedBatch>, JournalError> {
        let root = root.as_ref();
        let node_id = read_node_id(root)?;
        let segments = list_segments(root, &node_id)?;
        if let Some(checkpoint) = checkpoint {
            if !segments.iter().any(|segment| {
                segment.epoch == checkpoint.epoch && segment.index == checkpoint.segment
            }) {
                return Err(JournalError::Corrupt(
                    "checkpoint segment is missing".to_string(),
                ));
            }
        }
        let frames = scan_frames(&node_id, &segments, cipher, checkpoint)?;
        committed_batches(frames)
    }
}

pub(super) fn max_observed_sequence(
    node_id: &str,
    segments: &[JournalSegment],
    cipher: &JournalCipher,
) -> Result<u64, JournalError> {
    Ok(scan_frames(node_id, segments, cipher, None)?
        .into_iter()
        .map(|located| located.frame.sequence)
        .max()
        .unwrap_or(0))
}

fn scan_frames(
    node_id: &str,
    segments: &[JournalSegment],
    cipher: &JournalCipher,
    checkpoint: Option<&JournalCheckpoint>,
) -> Result<Vec<LocatedFrame>, JournalError> {
    let mut max_index_by_epoch = HashMap::<String, u32>::new();
    for segment in segments {
        max_index_by_epoch
            .entry(segment.epoch.clone())
            .and_modify(|index| *index = (*index).max(segment.index))
            .or_insert(segment.index);
    }

    let mut frames = Vec::new();
    for segment in segments {
        let mut start_offset = 0_usize;
        let mut skip_first_frame = false;
        if let Some(checkpoint) = checkpoint {
            let segment_position = (segment.epoch.as_str(), segment.index);
            let checkpoint_position = (checkpoint.epoch.as_str(), checkpoint.segment);
            if segment_position < checkpoint_position {
                continue;
            }
            if segment_position == checkpoint_position {
                start_offset = usize::try_from(checkpoint.offset).map_err(|_| {
                    JournalError::Corrupt("checkpoint offset is too large".to_string())
                })?;
                skip_first_frame = true;
            }
        }
        let bytes = fs::read(&segment.path)?;
        let binding = JournalBinding {
            node_id: node_id.to_string(),
            epoch: segment.epoch.clone(),
        };
        let scan = scan_segment(
            segment,
            &bytes,
            cipher,
            &binding,
            start_offset,
            skip_first_frame,
            checkpoint,
        )?;
        if scan.truncated_tail && max_index_by_epoch.get(&segment.epoch) != Some(&segment.index) {
            return Err(JournalError::Corrupt(format!(
                "truncated journal segment is not the last segment: {}",
                segment.path.display()
            )));
        }
        frames.extend(scan.frames);
    }
    frames.sort_by_key(|located| located.frame.sequence);
    for pair in frames.windows(2) {
        if pair[0].frame.sequence == pair[1].frame.sequence {
            return Err(JournalError::Corrupt(format!(
                "duplicate journal sequence {}",
                pair[0].frame.sequence
            )));
        }
    }
    Ok(frames)
}

struct SegmentScan {
    frames: Vec<LocatedFrame>,
    truncated_tail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocatedFrame {
    frame: Frame,
    epoch: String,
    segment: u32,
    offset: u64,
}

fn scan_segment(
    segment: &JournalSegment,
    bytes: &[u8],
    cipher: &JournalCipher,
    binding: &JournalBinding,
    start_offset: usize,
    skip_first_frame: bool,
    checkpoint: Option<&JournalCheckpoint>,
) -> Result<SegmentScan, JournalError> {
    let mut frames = Vec::new();
    if start_offset > bytes.len() {
        return Err(JournalError::Corrupt(
            "checkpoint offset is beyond the journal segment".to_string(),
        ));
    }
    let mut cursor = start_offset;
    let mut truncated_tail = false;
    let mut first_frame = true;
    while cursor < bytes.len() {
        if bytes.len() - cursor < 4 {
            truncated_tail = true;
            break;
        }
        let frame_offset = cursor;
        let length = u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .expect("four-byte length prefix"),
        ) as usize;
        cursor += 4;
        if length > MAX_FRAME_BYTES {
            return Err(JournalError::Corrupt(
                "journal frame length exceeds maximum".to_string(),
            ));
        }
        if bytes.len() - cursor < length {
            truncated_tail = true;
            break;
        }
        let frame = decode_frame(&bytes[cursor..cursor + length], cipher, binding)?;
        cursor += length;
        if skip_first_frame && first_frame {
            first_frame = false;
            if frame.frame_type != FrameType::Commit {
                return Err(JournalError::Corrupt(
                    "checkpoint offset does not point at a commit frame".to_string(),
                ));
            }
            let metadata: CommitMetadata =
                serde_json::from_slice(&frame.payload).map_err(|error| {
                    JournalError::Corrupt(format!("invalid commit frame payload: {error}"))
                })?;
            if checkpoint
                .is_none_or(|checkpoint| metadata.last_sequence > checkpoint.last_applied_sequence)
            {
                return Err(JournalError::Corrupt(
                    "checkpoint does not cover the commit batch".to_string(),
                ));
            }
            continue;
        }
        first_frame = false;
        frames.push(LocatedFrame {
            frame,
            epoch: segment.epoch.clone(),
            segment: segment.index,
            offset: frame_offset as u64,
        });
    }
    for pair in frames.windows(2) {
        if pair[0].frame.sequence >= pair[1].frame.sequence {
            return Err(JournalError::Corrupt(
                "journal sequences are not increasing within a segment".to_string(),
            ));
        }
    }
    Ok(SegmentScan {
        frames,
        truncated_tail,
    })
}

fn committed_batches(frames: Vec<LocatedFrame>) -> Result<Vec<CommittedBatch>, JournalError> {
    let mut pending = Vec::<LocatedFrame>::new();
    let mut batches = Vec::new();
    for located in frames {
        match located.frame.frame_type {
            FrameType::Data => pending.push(located),
            FrameType::Commit => {
                let metadata: CommitMetadata = serde_json::from_slice(&located.frame.payload)
                    .map_err(|error| {
                        JournalError::Corrupt(format!("invalid commit frame payload: {error}"))
                    })?;
                validate_commit(&located.frame, &metadata)?;
                pending.retain(|record| record.frame.sequence >= metadata.first_sequence);
                let expected =
                    usize::try_from(metadata.last_sequence - metadata.first_sequence + 1).map_err(
                        |_| JournalError::Corrupt("commit sequence range is too large".to_string()),
                    )?;
                if pending.len() < expected {
                    return Err(JournalError::Corrupt(
                        "commit frame does not cover complete records".to_string(),
                    ));
                }
                let selected = pending.drain(..expected).collect::<Vec<_>>();
                for (offset, record) in selected.iter().enumerate() {
                    if record.frame.sequence != metadata.first_sequence + offset as u64 {
                        return Err(JournalError::Corrupt(
                            "commit sequence range has a gap".to_string(),
                        ));
                    }
                }
                let records = selected.into_iter().map(|located| located.frame).collect();
                batches.push(CommittedBatch {
                    metadata,
                    records,
                    position: BatchPosition {
                        epoch: located.epoch,
                        segment: located.segment,
                        offset: located.offset,
                    },
                });
            }
        }
    }
    Ok(batches)
}

fn validate_commit(frame: &Frame, metadata: &CommitMetadata) -> Result<(), JournalError> {
    if metadata.group_id.is_empty() || metadata.committed_at.is_empty() {
        return Err(JournalError::Corrupt(
            "commit frame metadata is incomplete".to_string(),
        ));
    }
    if metadata.first_sequence == 0
        || metadata.first_sequence > metadata.last_sequence
        || frame.sequence
            != metadata
                .last_sequence
                .checked_add(1)
                .ok_or_else(|| JournalError::Corrupt("commit sequence overflow".to_string()))?
    {
        return Err(JournalError::Corrupt(
            "commit frame has an invalid sequence range".to_string(),
        ));
    }
    Ok(())
}
