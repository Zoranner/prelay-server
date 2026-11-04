use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

mod cache;
mod index;
mod read;
mod scan;
mod support;
mod writer;

use self::cache::ThreadIndex;
use crate::domain::activity::rollout::{
    ContentEvent, ContentEventBody, ConversationEvent, ConversationFormat, ConversationMeta,
    RolloutError, DEFAULT_ACTIVITY_ROLLOUT_DIR,
};

#[derive(Debug, Clone)]
pub struct RolloutStore {
    root: PathBuf,
    locks: Arc<Mutex<HashMap<PathBuf, std::sync::Weak<AsyncMutex<()>>>>>,
    cache: Arc<Mutex<ThreadIndex>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RolloutFile {
    pub format: ConversationFormat,
    pub meta: Option<ConversationMeta>,
    pub events: Vec<ConversationEvent>,
    pub event_count: u64,
    pub byte_size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RolloutSlice {
    pub events: Vec<ConversationEvent>,
    pub event_count: u64,
    pub byte_size: u64,
    pub sha256: String,
}

impl RolloutFile {
    pub fn content_events(&self) -> Vec<ContentEvent> {
        self.events
            .iter()
            .filter_map(ConversationEvent::content_event)
            .collect()
    }

    pub fn memory_events(&self) -> Vec<ContentEvent> {
        self.content_events()
            .into_iter()
            .filter(|event| {
                !matches!(
                    &event.body,
                    ContentEventBody::Error(_) | ContentEventBody::Usage(_)
                )
            })
            .collect()
    }

    pub fn resolve_refs(&self, refs: &[u64]) -> Vec<&ConversationEvent> {
        refs.iter()
            .filter_map(|seq| self.events.iter().find(|event| event.seq == *seq))
            .collect()
    }

    pub fn context_refs_for_turn(&self, turn_id: &str) -> Option<Vec<u64>> {
        let mut matched = false;
        for event in &self.events {
            if event.event_type == "turn"
                && event
                    .payload
                    .get("turn_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(turn_id)
            {
                matched = true;
            } else if matched && event.event_type == "context" {
                return event
                    .payload
                    .get("refs")
                    .and_then(serde_json::Value::as_array)
                    .map(|refs| {
                        refs.iter()
                            .filter_map(serde_json::Value::as_u64)
                            .collect::<Vec<_>>()
                    });
            }
        }
        None
    }
}

impl RolloutStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            locks: Arc::new(Mutex::new(HashMap::new())),
            cache: Arc::new(Mutex::new(ThreadIndex::default())),
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

    pub(super) fn lock_for(&self, path: &Path) -> Arc<AsyncMutex<()>> {
        let mut locks = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(path).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
        lock
    }
}

pub use writer::RolloutWriter;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod thread_tests;

#[cfg(test)]
mod thread_cache_tests;

#[cfg(test)]
mod conversation_tests;

#[cfg(test)]
mod benchmark_tests;
