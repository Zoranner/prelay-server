use std::{
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    event::{RolloutEvent, RolloutEventBody, SessionEndEvent, SessionMetaEvent},
    RolloutError, RolloutStatus, RolloutSummary, DEFAULT_ACTIVITY_ROLLOUT_DIR,
};
use crate::activity::redaction::redact_json_value;

#[derive(Debug, Clone)]
pub struct RolloutStore {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RolloutFile {
    pub events: Vec<RolloutEvent>,
    pub event_count: u64,
    pub byte_size: u64,
    pub sha256: String,
}

impl RolloutStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn from_environment() -> Result<Self, String> {
        let root = std::env::var("ACTIVITY_ROLLOUT_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_ACTIVITY_ROLLOUT_DIR.to_string());
        let root = PathBuf::from(root);
        if root.as_os_str().is_empty() {
            return Err("ACTIVITY_ROLLOUT_DIR must not be empty".to_string());
        }
        Ok(Self { root })
    }

    /// Ensure the configured root exists and is writable before the service starts accepting
    /// traffic. A read-only bind mount must fail at startup, not on the first streamed request.
    pub fn validate(&self) -> Result<(), RolloutError> {
        fs::create_dir_all(&self.root)?;
        let probe = self
            .root
            .join(format!(".prelay-write-check-{}", Uuid::new_v4()));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&probe)?;
        drop(file);
        fs::remove_file(probe)?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn absolute_path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn checked_absolute_path(&self, relative: &str) -> Result<PathBuf, RolloutError> {
        if relative.is_empty()
            || Path::new(relative).is_absolute()
            || relative.starts_with('/')
            || relative.starts_with('\\')
            || relative.contains('\\')
            || relative.contains(':')
            || relative
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            return Err(RolloutError::InvalidPath(relative.to_string()));
        }
        Ok(self.absolute_path(relative))
    }

    pub fn begin(
        &self,
        meta: SessionMetaEvent,
        request_events: Vec<RolloutEventBody>,
    ) -> Result<RolloutWriter, RolloutError> {
        let now = Utc::now();
        let relative_path = format!(
            "{}/{}/rollout-{}-{}.jsonl",
            now.format("%Y"),
            now.format("%m/%d"),
            now.format("%Y-%m-%dT%H-%M-%S"),
            meta.activity_id
        );
        let final_path = self.absolute_path(&relative_path);
        let parent = final_path
            .parent()
            .ok_or_else(|| RolloutError::InvalidPath(relative_path.clone()))?;
        fs::create_dir_all(parent)?;
        let part_path = PathBuf::from(format!("{}.part", final_path.display()));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&part_path)?;
        let mut writer = RolloutWriter {
            activity_id: meta.activity_id.clone(),
            session_id: meta.session_id.clone(),
            relative_path,
            part_path,
            final_path,
            file: Some(BufWriter::new(file)),
            hasher: Sha256::new(),
            seq: 0,
            byte_size: 0,
            event_count: 0,
            status: RolloutStatus::Writing,
            finished: false,
        };
        writer.append(RolloutEventBody::SessionMeta(meta))?;
        for event in request_events {
            writer.append(event)?;
        }
        Ok(writer)
    }

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

    pub fn part_files(&self) -> Result<Vec<PathBuf>, RolloutError> {
        let mut files = Vec::new();
        collect_part_files(&self.root, &mut files)?;
        Ok(files)
    }

    pub fn rollout_files(&self) -> Result<Vec<String>, RolloutError> {
        let mut files = Vec::new();
        collect_rollout_files(&self.root, &self.root, &mut files)?;
        Ok(files)
    }
}

fn collect_part_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), RolloutError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_part_files(&entry.path(), files)?;
        } else if file_type.is_file() && entry.path().extension().is_some_and(|ext| ext == "part") {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn collect_rollout_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<String>,
) -> Result<(), RolloutError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_rollout_files(root, &path, files)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| RolloutError::InvalidPath(path.display().to_string()))?;
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

pub struct RolloutWriter {
    activity_id: String,
    session_id: String,
    relative_path: String,
    part_path: PathBuf,
    final_path: PathBuf,
    file: Option<BufWriter<std::fs::File>>,
    hasher: Sha256,
    seq: u64,
    byte_size: u64,
    event_count: u64,
    status: RolloutStatus,
    finished: bool,
}

impl RolloutWriter {
    pub fn append(&mut self, body: RolloutEventBody) -> Result<(), RolloutError> {
        if self.finished {
            return Err(RolloutError::InvalidPath(
                "rollout is already finished".to_string(),
            ));
        }
        let body_value = serde_json::to_value(body)?;
        let object = body_value
            .as_object()
            .ok_or_else(|| RolloutError::InvalidPath("event body is not an object".to_string()))?;
        let event_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| RolloutError::InvalidPath("event type is missing".to_string()))?;
        let payload = object.get("payload").cloned().unwrap_or(Value::Null);
        self.seq += 1;
        let event = RolloutEvent {
            seq: self.seq,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            event_type: event_type.to_string(),
            payload: redact_json_value(&payload),
        };
        let mut line = serde_json::to_vec(&event)?;
        line.push(b'\n');
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| RolloutError::InvalidPath("rollout file is closed".to_string()))?;
        file.write_all(&line)?;
        file.flush()?;
        self.hasher.update(&line);
        self.byte_size += line.len() as u64;
        self.event_count += 1;
        Ok(())
    }

    pub fn summary(&self) -> RolloutSummary {
        self.summary_with_status(self.status)
    }

    pub fn finish(&mut self, status: RolloutStatus) -> Result<RolloutSummary, RolloutError> {
        self.finish_with(status, None, None, None)
    }

    pub fn finish_with(
        &mut self,
        status: RolloutStatus,
        http_status: Option<u16>,
        error_code: Option<String>,
        error_message: Option<String>,
    ) -> Result<RolloutSummary, RolloutError> {
        if self.finished {
            return Ok(self.summary_with_status(self.status));
        }
        self.append(RolloutEventBody::SessionEnd(SessionEndEvent {
            status,
            http_status,
            error_code,
            error_message,
        }))?;
        let mut file = self
            .file
            .take()
            .ok_or_else(|| RolloutError::InvalidPath("rollout file is closed".to_string()))?;
        file.flush()?;
        file.get_ref().sync_all()?;
        fs::rename(&self.part_path, &self.final_path)?;
        self.status = status;
        self.finished = true;
        Ok(self.summary_with_status(self.status))
    }

    fn summary_with_status(&self, status: RolloutStatus) -> RolloutSummary {
        RolloutSummary {
            activity_id: self.activity_id.clone(),
            session_id: self.session_id.clone(),
            relative_path: self.relative_path.clone(),
            status,
            event_count: self.event_count,
            byte_size: self.byte_size,
            sha256: format!("{:x}", self.hasher.clone().finalize()),
        }
    }
}
