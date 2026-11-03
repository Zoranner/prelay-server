use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};

use sha2::{Digest, Sha256};

use super::{
    cache::{FileStamp, ThreadState},
    RolloutFile, RolloutSlice, RolloutStore,
};
use crate::domain::activity::rollout::{RolloutError, RolloutEvent};

pub(super) const MAX_RECOVERED_SEGMENTS: usize = 10;

enum ThreadCandidate {
    Missing,
    Corrupt,
    Events(Vec<RolloutEvent>, FileStamp),
}

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

    pub(super) fn load_thread_source(
        &self,
        identity_id: &str,
        thread_id: &str,
    ) -> Result<(ThreadState, Vec<RolloutEvent>), RolloutError> {
        let base = format!("{identity_id}/{thread_id}.jsonl");
        match self.read_thread_candidate(&base)? {
            ThreadCandidate::Events(events, stamp) => {
                let state = ThreadState::from_events(base, stamp, &events)?;
                Ok((state, events))
            }
            ThreadCandidate::Missing => {
                for index in 1..=MAX_RECOVERED_SEGMENTS {
                    let candidate = recovered_thread_path(identity_id, thread_id, index);
                    match self.read_thread_candidate(&candidate)? {
                        ThreadCandidate::Events(events, stamp) => {
                            let state = ThreadState::from_events(candidate, stamp, &events)?;
                            return Ok((state, events));
                        }
                        ThreadCandidate::Corrupt => continue,
                        ThreadCandidate::Missing if index == 1 => {
                            return Ok((ThreadState::empty(base), Vec::new()));
                        }
                        ThreadCandidate::Missing => {
                            return Ok((ThreadState::empty(candidate), Vec::new()));
                        }
                    }
                }
                Err(RolloutError::InvalidPath(format!(
                    "thread {thread_id} has too many corrupt recovered segments"
                )))
            }
            ThreadCandidate::Corrupt => {
                for index in 1..=MAX_RECOVERED_SEGMENTS {
                    let candidate = recovered_thread_path(identity_id, thread_id, index);
                    match self.read_thread_candidate(&candidate)? {
                        ThreadCandidate::Events(events, stamp) => {
                            let state = ThreadState::from_events(candidate, stamp, &events)?;
                            return Ok((state, events));
                        }
                        ThreadCandidate::Corrupt => continue,
                        ThreadCandidate::Missing => {
                            return Ok((ThreadState::empty(candidate), Vec::new()));
                        }
                    }
                }
                Err(RolloutError::InvalidPath(format!(
                    "thread {thread_id} has too many corrupt segments"
                )))
            }
        }
    }

    fn read_thread_candidate(&self, relative: &str) -> Result<ThreadCandidate, RolloutError> {
        let path = self.checked_absolute_path(relative)?;
        match read_thread_events(&path) {
            Ok(Some(events)) => {
                let stamp = FileStamp::from_metadata(&fs::metadata(&path)?);
                Ok(ThreadCandidate::Events(events, stamp))
            }
            Ok(None) => Ok(ThreadCandidate::Missing),
            Err(RolloutError::Serialize(_)) => {
                tracing::warn!(
                    thread = %relative,
                    "activity rollout has a corrupt line; trying a recovered segment"
                );
                Ok(ThreadCandidate::Corrupt)
            }
            Err(error) => Err(error),
        }
    }
}

fn recovered_thread_path(identity_id: &str, thread_id: &str, index: usize) -> String {
    if index == 1 {
        format!("{identity_id}/{thread_id}.recovered.jsonl")
    } else {
        format!("{identity_id}/{thread_id}.recovered-{index}.jsonl")
    }
}

pub(super) fn thread_candidate_paths(identity_id: &str, thread_id: &str) -> Vec<String> {
    let mut paths = Vec::with_capacity(MAX_RECOVERED_SEGMENTS + 1);
    paths.push(format!("{identity_id}/{thread_id}.jsonl"));
    for index in 1..=MAX_RECOVERED_SEGMENTS {
        paths.push(recovered_thread_path(identity_id, thread_id, index));
    }
    paths
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
