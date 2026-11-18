use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{cipher::JournalCipher, JournalError};

pub(crate) const MAGIC: &str = "PRW1";
pub(crate) const VERSION: u8 = 1;
pub(crate) const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalBinding {
    pub(crate) node_id: String,
    pub(crate) epoch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameType {
    Data,
    Commit,
}

impl FrameType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Commit => "commit",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "data" => Some(Self::Data),
            "commit" => Some(Self::Commit),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) sequence: u64,
    pub(crate) frame_type: FrameType,
    pub(crate) activity_id: String,
    pub(crate) kind: String,
    pub(crate) part_index: u32,
    pub(crate) part_count: u32,
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CommitMetadata {
    pub(crate) group_id: String,
    pub(crate) first_sequence: u64,
    pub(crate) last_sequence: u64,
    pub(crate) committed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommittedBatch {
    pub(crate) metadata: CommitMetadata,
    pub(crate) records: Vec<Frame>,
    pub(crate) position: BatchPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchPosition {
    pub(crate) epoch: String,
    pub(crate) segment: u32,
    pub(crate) offset: u64,
}

impl Frame {
    fn validate(&self) -> Result<(), JournalError> {
        if self.sequence == 0 {
            return Err(JournalError::Corrupt(
                "journal frame sequence must be positive".to_string(),
            ));
        }
        if self.kind.is_empty() {
            return Err(JournalError::Corrupt(
                "journal frame kind is empty".to_string(),
            ));
        }
        match self.frame_type {
            FrameType::Data => {
                if self.activity_id.is_empty() {
                    return Err(JournalError::Corrupt(
                        "journal data frame has no activity ID".to_string(),
                    ));
                }
                if self.part_count == 0 || self.part_index >= self.part_count {
                    return Err(JournalError::Corrupt(
                        "journal data frame has an invalid part range".to_string(),
                    ));
                }
            }
            FrameType::Commit => {
                if !self.activity_id.is_empty() || self.part_index != 0 || self.part_count != 1 {
                    return Err(JournalError::Corrupt(
                        "journal commit frame has data metadata".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn aad(&self, binding: &JournalBinding) -> Vec<u8> {
        format!(
            "{MAGIC}\n{VERSION}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            binding.node_id,
            binding.epoch,
            self.frame_type.as_str(),
            self.sequence,
            self.activity_id,
            self.kind,
            self.part_index,
            self.part_count
        )
        .into_bytes()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredFrame {
    magic: String,
    version: u8,
    frame_type: String,
    sequence: u64,
    activity_id: String,
    kind: String,
    part_index: u32,
    part_count: u32,
    nonce: String,
    ciphertext: String,
    checksum: String,
}

pub(crate) fn encode_frame(
    frame: &Frame,
    cipher: &JournalCipher,
    binding: &JournalBinding,
) -> Result<Vec<u8>, JournalError> {
    frame.validate()?;
    let sealed = cipher.seal(&frame.payload, &frame.aad(binding))?;
    let mut checksum_input = Vec::with_capacity(12 + sealed.ciphertext.len());
    checksum_input.extend_from_slice(&sealed.nonce);
    checksum_input.extend_from_slice(&sealed.ciphertext);
    let stored = StoredFrame {
        magic: MAGIC.to_string(),
        version: VERSION,
        frame_type: frame.frame_type.as_str().to_string(),
        sequence: frame.sequence,
        activity_id: frame.activity_id.clone(),
        kind: frame.kind.clone(),
        part_index: frame.part_index,
        part_count: frame.part_count,
        nonce: STANDARD.encode(sealed.nonce),
        ciphertext: STANDARD.encode(&sealed.ciphertext),
        checksum: STANDARD.encode(Sha256::digest(checksum_input)),
    };
    Ok(serde_json::to_vec(&stored)?)
}

pub(crate) fn decode_frame(
    encoded: &[u8],
    cipher: &JournalCipher,
    binding: &JournalBinding,
) -> Result<Frame, JournalError> {
    let stored: StoredFrame = serde_json::from_slice(encoded)
        .map_err(|error| JournalError::Corrupt(format!("invalid journal frame JSON: {error}")))?;
    if stored.magic != MAGIC {
        return Err(JournalError::Corrupt("unknown journal magic".to_string()));
    }
    if stored.version != VERSION {
        return Err(JournalError::Corrupt(format!(
            "unsupported journal version {}",
            stored.version
        )));
    }
    let frame_type = FrameType::parse(&stored.frame_type)
        .ok_or_else(|| JournalError::Corrupt("unknown journal frame type".to_string()))?;
    let nonce = STANDARD
        .decode(&stored.nonce)
        .map_err(|_| JournalError::Corrupt("journal nonce is not base64".to_string()))?;
    let ciphertext = STANDARD
        .decode(&stored.ciphertext)
        .map_err(|_| JournalError::Corrupt("journal ciphertext is not base64".to_string()))?;
    let actual_checksum = STANDARD
        .decode(&stored.checksum)
        .map_err(|_| JournalError::Corrupt("journal checksum is not base64".to_string()))?;
    let mut checksum_input = Vec::with_capacity(nonce.len() + ciphertext.len());
    checksum_input.extend_from_slice(&nonce);
    checksum_input.extend_from_slice(&ciphertext);
    if actual_checksum.as_slice() != Sha256::digest(checksum_input).as_slice() {
        return Err(JournalError::Corrupt(
            "journal frame checksum does not match".to_string(),
        ));
    }
    let frame = Frame {
        sequence: stored.sequence,
        frame_type,
        activity_id: stored.activity_id,
        kind: stored.kind,
        part_index: stored.part_index,
        part_count: stored.part_count,
        payload: Vec::new(),
    };
    frame.validate()?;
    let payload = cipher.open(&nonce, &ciphertext, &frame.aad(binding))?;
    Ok(Frame { payload, ..frame })
}
