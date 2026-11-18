use std::{fs, io::Write, path::Path};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use uuid::Uuid;

use super::JournalError;

const CHECKPOINT_FILE: &str = "checkpoint.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct JournalCheckpoint {
    pub(crate) node_id: String,
    pub(crate) epoch: String,
    pub(crate) segment: u32,
    pub(crate) offset: u64,
    pub(crate) last_applied_sequence: u64,
    pub(crate) updated_at: String,
}

impl JournalCheckpoint {
    pub(crate) fn load(root: &Path, expected_node_id: &str) -> Result<Option<Self>, JournalError> {
        let path = root.join(CHECKPOINT_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let checkpoint = serde_json::from_slice::<Self>(&bytes)
            .map_err(|error| JournalError::Corrupt(format!("invalid checkpoint JSON: {error}")))?;
        checkpoint.validate(expected_node_id)?;
        Ok(Some(checkpoint))
    }

    pub(crate) fn save(&self, root: &Path) -> Result<(), JournalError> {
        self.validate(&self.node_id)?;
        let bytes = serde_json::to_vec(self)?;
        let mut temporary = NamedTempFile::new_in(root)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(root.join(CHECKPOINT_FILE))
            .map_err(|error| JournalError::Io(error.error))?;
        Ok(())
    }

    fn validate(&self, expected_node_id: &str) -> Result<(), JournalError> {
        if self.node_id != expected_node_id {
            return Err(JournalError::InvalidLayout(
                "checkpoint belongs to another node".to_string(),
            ));
        }
        Uuid::parse_str(&self.epoch).map_err(|_| {
            JournalError::Corrupt("checkpoint epoch is not a valid UUID".to_string())
        })?;
        if self.updated_at.is_empty() {
            return Err(JournalError::Corrupt(
                "checkpoint updated_at is empty".to_string(),
            ));
        }
        Ok(())
    }
}
