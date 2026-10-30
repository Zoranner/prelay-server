use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};

use sha2::{Digest, Sha256};

use super::{RolloutFile, RolloutSlice, RolloutStore};
use crate::domain::activity::rollout::{RolloutError, RolloutEvent};

impl RolloutStore {
    pub fn read_events(&self, relative_path: &str) -> Result<Vec<RolloutEvent>, RolloutError> {
        self.read_verified(relative_path).map(|file| file.events)
    }

    pub fn read_verified(&self, relative_path: &str) -> Result<RolloutFile, RolloutError> {
        let path = self.checked_absolute_path(relative_path)?;
        let contents = fs::read_to_string(path)?;
        let complete = contents.ends_with('\n') || contents.is_empty();
        if !complete {
            let valid_events = contents
                .lines()
                .take_while(|line| serde_json::from_str::<RolloutEvent>(line).is_ok())
                .count();
            return Err(RolloutError::PartialLine {
                path: self.checked_absolute_path(relative_path)?,
                valid_events,
            });
        }
        let events = contents
            .lines()
            .map(|line| serde_json::from_str::<RolloutEvent>(line).map_err(RolloutError::from))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RolloutFile {
            event_count: events.len() as u64,
            byte_size: contents.len() as u64,
            sha256: format!("{:x}", Sha256::digest(contents.as_bytes())),
            events,
        })
    }

    pub fn read_slice(
        &self,
        relative_path: &str,
        start_line: u64,
        end_line: u64,
    ) -> Result<RolloutSlice, RolloutError> {
        let path = self.checked_absolute_path(relative_path)?;
        let contents = fs::read_to_string(&path)?;
        let complete = contents.ends_with('\n') || contents.is_empty();
        if !complete {
            let valid_events = contents
                .lines()
                .take_while(|line| serde_json::from_str::<RolloutEvent>(line).is_ok())
                .count();
            return Err(RolloutError::PartialLine { path, valid_events });
        }
        let lines = contents.lines().collect::<Vec<_>>();
        if start_line == 0 {
            if end_line == 0 {
                return Ok(empty_slice());
            }
            return Err(RolloutError::InvalidPath(format!(
                "invalid rollout line range {start_line}..{end_line}"
            )));
        }
        if start_line > end_line {
            if start_line == end_line.saturating_add(1) && start_line <= lines.len() as u64 + 1 {
                return Ok(empty_slice());
            }
            return Err(RolloutError::InvalidPath(format!(
                "invalid rollout line range {start_line}..{end_line}"
            )));
        }
        if end_line > lines.len() as u64 || start_line > lines.len() as u64 + 1 {
            return Err(RolloutError::InvalidPath(format!(
                "rollout line range {start_line}..{end_line} exceeds {} lines",
                lines.len()
            )));
        }

        let selected = &lines[start_line as usize - 1..end_line as usize];
        let mut bytes = Vec::new();
        for line in selected {
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
        }
        let events = selected
            .iter()
            .map(|line| serde_json::from_str::<RolloutEvent>(line).map_err(RolloutError::from))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RolloutSlice {
            event_count: events.len() as u64,
            byte_size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            events,
        })
    }
}

fn empty_slice() -> RolloutSlice {
    RolloutSlice {
        events: Vec::new(),
        event_count: 0,
        byte_size: 0,
        sha256: format!("{:x}", Sha256::digest(b"")),
    }
}

pub(super) fn parse_events(contents: &str) -> Result<Vec<RolloutEvent>, RolloutError> {
    contents
        .lines()
        .map(|line| serde_json::from_str::<RolloutEvent>(line).map_err(RolloutError::from))
        .collect()
}

pub(super) fn completed_contents(contents: &str) -> bool {
    contents.ends_with('\n') || contents.is_empty()
}

pub(super) fn read_thread_events(path: &Path) -> Result<Option<Vec<RolloutEvent>>, RolloutError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if completed_contents(&contents) {
        return parse_events(&contents).map(Some);
    }

    let valid_len = contents.rfind('\n').map_or(0, |index| index + 1);
    let events = parse_events(&contents[..valid_len])?;
    let dropped_bytes = contents.len() - valid_len;
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(valid_len as u64)?;
    file.sync_all()?;
    tracing::warn!(
        path = %path.display(),
        dropped_bytes,
        "repaired an incomplete activity rollout tail"
    );
    Ok(Some(events))
}
