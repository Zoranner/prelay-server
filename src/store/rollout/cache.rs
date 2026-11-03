use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::{
    support::{last_window_generation, ContentHash},
    RolloutStore,
};
use crate::domain::activity::rollout::{RolloutError, RolloutEvent, SessionContext};

const MAX_CACHED_THREADS: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct FileStamp {
    pub(super) len: u64,
    pub(super) modified: Option<SystemTime>,
}

impl FileStamp {
    pub(super) fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }
}

pub(super) enum ThreadSource {
    Cached(ThreadState),
    Loaded(ThreadState, Vec<RolloutEvent>),
}

#[derive(Debug, Clone)]
pub(super) struct ThreadState {
    pub(super) relative_path: String,
    pub(super) stamp: FileStamp,
    pub(super) line_count: u64,
    pub(super) seq: u64,
    pub(super) generation: Option<u64>,
    pub(super) content: ContentHash,
}

impl ThreadState {
    pub(super) fn empty(relative_path: String) -> Self {
        Self {
            relative_path,
            stamp: FileStamp::default(),
            line_count: 0,
            seq: 0,
            generation: None,
            content: ContentHash::default(),
        }
    }

    pub(super) fn from_events(
        relative_path: String,
        stamp: FileStamp,
        events: &[RolloutEvent],
    ) -> Result<Self, RolloutError> {
        Ok(Self {
            relative_path,
            stamp,
            line_count: events.len() as u64,
            seq: events.last().map_or(0, |event| event.seq),
            generation: last_window_generation(events),
            content: ContentHash::from_events(events)?,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.line_count == 0
    }

    pub(super) fn window_changed(&self, session: &SessionContext) -> bool {
        match self.generation {
            Some(generation) => generation != session.generation,
            None => !self.is_empty() && session.generation > 0,
        }
    }
}

#[derive(Debug)]
struct ThreadEntry {
    state: ThreadState,
    last_used: u64,
}

#[derive(Debug)]
pub(super) struct ThreadIndex {
    entries: HashMap<PathBuf, ThreadEntry>,
    clock: u64,
    max_entries: usize,
}

impl Default for ThreadIndex {
    fn default() -> Self {
        Self::with_limit(MAX_CACHED_THREADS)
    }
}

impl ThreadIndex {
    fn with_limit(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            max_entries,
        }
    }

    fn get(&mut self, key: &Path) -> Option<ThreadState> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.last_used = self.clock;
        Some(entry.state.clone())
    }

    pub(super) fn insert(&mut self, key: PathBuf, state: ThreadState) {
        self.clock = self.clock.wrapping_add(1);
        if self.entries.len() >= self.max_entries && !self.entries.contains_key(&key) {
            if let Some(evicted) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            {
                self.entries.remove(&evicted);
            }
        }
        self.entries.insert(
            key,
            ThreadEntry {
                state,
                last_used: self.clock,
            },
        );
    }

    fn remove_paths(&mut self, relative_paths: &[String]) {
        let paths = relative_paths
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        self.entries
            .retain(|_, entry| !paths.contains(entry.state.relative_path.as_str()));
    }
}

impl RolloutStore {
    pub(super) fn resolve_thread_source(
        &self,
        identity_id: &str,
        thread_id: &str,
    ) -> Result<ThreadSource, RolloutError> {
        let key = PathBuf::from(format!("{identity_id}/{thread_id}"));
        if let Some(state) = self.cached_thread(&key) {
            let path = self.checked_absolute_path(&state.relative_path)?;
            if fs::metadata(path)
                .is_ok_and(|metadata| state.stamp == FileStamp::from_metadata(&metadata))
            {
                return Ok(ThreadSource::Cached(state));
            }
        }
        if let Some(state) = self.load_persisted_thread(identity_id, thread_id)? {
            self.cache_thread(key, state.clone());
            return Ok(ThreadSource::Cached(state));
        }

        let (state, events) = self.load_thread_source(identity_id, thread_id)?;
        self.cache_thread(key, state.clone());
        self.persist_thread_index(identity_id, thread_id, &state);
        Ok(ThreadSource::Loaded(state, events))
    }

    pub(super) fn cache_thread(&self, key: PathBuf, state: ThreadState) {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, state);
    }

    fn cached_thread(&self, key: &Path) -> Option<ThreadState> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
    }

    pub(crate) fn forget_relative_paths(&self, relative_paths: &[String]) {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove_paths(relative_paths);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{FileStamp, ThreadIndex, ThreadState};

    #[test]
    fn file_stamp_detects_length_and_mtime_changes() {
        let base = FileStamp {
            len: 10,
            modified: Some(SystemTime::UNIX_EPOCH),
        };
        let same = FileStamp {
            len: 10,
            modified: Some(SystemTime::UNIX_EPOCH),
        };
        let longer = FileStamp {
            len: 11,
            modified: Some(SystemTime::UNIX_EPOCH),
        };
        let newer = FileStamp {
            len: 10,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
        };

        assert_eq!(base, same);
        assert_ne!(base, longer);
        assert_ne!(base, newer);
    }

    #[test]
    fn index_evicts_the_least_recently_used_thread() {
        let mut index = ThreadIndex::with_limit(2);
        index.insert("a".into(), ThreadState::empty("a".to_string()));
        index.insert("b".into(), ThreadState::empty("b".to_string()));
        assert!(index.get("a".as_ref()).is_some());

        index.insert("c".into(), ThreadState::empty("c".to_string()));

        assert!(index.get("a".as_ref()).is_some());
        assert!(index.get("b".as_ref()).is_none());
        assert!(index.get("c".as_ref()).is_some());
    }

    #[test]
    fn index_removes_all_paths_owned_by_a_thread() {
        let mut index = ThreadIndex::with_limit(4);
        index.insert("a".into(), ThreadState::empty("a.jsonl".to_string()));
        index.insert("b".into(), ThreadState::empty("b.jsonl".to_string()));

        index.remove_paths(&["a.jsonl".to_string()]);

        assert!(index.get("a".as_ref()).is_none());
        assert!(index.get("b".as_ref()).is_some());
    }
}
