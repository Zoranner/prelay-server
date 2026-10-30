use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chrono::{SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

mod read;
mod scan;
mod support;

use self::read::{completed_contents, parse_events, valid_prefix};
use self::support::{common_prefix_len, event_parts, is_safe_path_segment, last_window_generation};
use crate::domain::activity::rollout::{
    RolloutError, RolloutEvent, RolloutEventBody, RolloutStatus, RolloutSummary, SessionContext,
    SessionMetaEvent, WindowStartEvent, DEFAULT_ACTIVITY_ROLLOUT_DIR,
};

#[derive(Debug, Clone)]
pub struct RolloutStore {
    root: PathBuf,
    locks: Arc<Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RolloutFile {
    pub events: Vec<RolloutEvent>,
    pub event_count: u64,
    pub byte_size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RolloutSlice {
    pub events: Vec<RolloutEvent>,
    pub event_count: u64,
    pub byte_size: u64,
    pub sha256: String,
}

impl RolloutStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
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
        Ok(Self::new(root))
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

    pub async fn begin(
        &self,
        identity_id: &str,
        session: &SessionContext,
        meta: SessionMetaEvent,
        request_events: Vec<RolloutEventBody>,
    ) -> Result<RolloutWriter, RolloutError> {
        if !is_safe_path_segment(identity_id) {
            return Err(RolloutError::InvalidPath(format!(
                "identity id is not a safe path segment: {identity_id}"
            )));
        }
        if !is_safe_path_segment(&session.thread_id) {
            return Err(RolloutError::InvalidPath(format!(
                "thread id is not a safe path segment: {}",
                session.thread_id
            )));
        }

        let relative_path = format!("{identity_id}/{}.jsonl", session.thread_id);
        let final_path = self.checked_absolute_path(&relative_path)?;
        let parent = final_path
            .parent()
            .ok_or_else(|| RolloutError::InvalidPath(relative_path.clone()))?;
        fs::create_dir_all(parent)?;

        let lock = self.lock_for(&final_path);
        let guard = lock.lock_owned().await;
        let existing_events = match fs::read_to_string(&final_path) {
            Ok(contents) => {
                if !completed_contents(&contents) {
                    return Err(RolloutError::PartialLine {
                        path: final_path,
                        valid_events: valid_prefix(&contents),
                    });
                }
                parse_events(&contents)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        let line_count = existing_events.len() as u64;
        let seq = existing_events.last().map_or(0, |event| event.seq);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&final_path)?;
        let mut writer = RolloutWriter {
            activity_id: meta.activity_id.clone(),
            thread_id: session.thread_id.clone(),
            window_id: session.window_id.clone(),
            generation: session.generation,
            relative_path,
            final_path,
            file: Some(BufWriter::new(file)),
            _lock: Some(guard),
            hasher: Sha256::new(),
            seq,
            line_count,
            start_line: line_count + 1,
            end_line: line_count,
            byte_size: 0,
            event_count: 0,
            status: RolloutStatus::Writing,
            finished: false,
        };

        let current_generation = last_window_generation(&existing_events);
        let window_changed = match current_generation {
            Some(generation) => generation != session.generation,
            None => !existing_events.is_empty() && session.generation > 0,
        };
        if existing_events.is_empty() {
            writer.append(RolloutEventBody::SessionMeta(meta))?;
            writer.append(RolloutEventBody::WindowStart(WindowStartEvent {
                window_id: session.window_id.clone(),
                generation: session.generation,
                started_at: Utc::now().to_rfc3339(),
            }))?;
        } else if window_changed {
            writer.append(RolloutEventBody::WindowStart(WindowStartEvent {
                window_id: session.window_id.clone(),
                generation: session.generation,
                started_at: Utc::now().to_rfc3339(),
            }))?;
        }

        let matched = if window_changed || existing_events.is_empty() {
            0
        } else {
            common_prefix_len(&existing_events, &request_events)?
        };
        for event in request_events.into_iter().skip(matched) {
            writer.append(event)?;
        }
        Ok(writer)
    }

    fn lock_for(&self, path: &Path) -> Arc<AsyncMutex<()>> {
        let mut locks = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locks.entry(path.to_path_buf()).or_default().clone()
    }
}

pub struct RolloutWriter {
    activity_id: String,
    thread_id: String,
    window_id: Option<String>,
    generation: u64,
    relative_path: String,
    final_path: PathBuf,
    file: Option<BufWriter<std::fs::File>>,
    _lock: Option<OwnedMutexGuard<()>>,
    hasher: Sha256,
    seq: u64,
    line_count: u64,
    start_line: u64,
    end_line: u64,
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
        let (event_type, payload) = event_parts(&body)?;
        self.seq += 1;
        let event = RolloutEvent {
            seq: self.seq,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            event_type,
            payload,
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
        self.line_count += 1;
        self.end_line = self.line_count;
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
        _http_status: Option<u16>,
        _error_code: Option<String>,
        _error_message: Option<String>,
    ) -> Result<RolloutSummary, RolloutError> {
        if self.finished {
            return Ok(self.summary_with_status(self.status));
        }
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            file.get_ref().sync_all()?;
        }
        self._lock.take();
        self.status = status;
        self.finished = true;
        Ok(self.summary_with_status(self.status))
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn final_path(&self) -> &Path {
        &self.final_path
    }

    fn summary_with_status(&self, status: RolloutStatus) -> RolloutSummary {
        RolloutSummary {
            activity_id: self.activity_id.clone(),
            thread_id: self.thread_id.clone(),
            window_id: self.window_id.clone(),
            generation: self.generation,
            relative_path: self.relative_path.clone(),
            status,
            start_line: self.start_line,
            end_line: self.end_line,
            event_count: self.event_count,
            byte_size: self.byte_size,
            sha256: format!("{:x}", self.hasher.clone().finalize()),
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod thread_tests;
