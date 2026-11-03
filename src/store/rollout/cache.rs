use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use super::{
    support::{last_window_generation, ContentChain},
    RolloutStore,
};
use crate::domain::activity::rollout::{RolloutError, RolloutEvent, SessionContext};

const MAX_CACHED_THREADS: usize = 65_536;

pub(super) enum ThreadSource {
    Cached(ThreadState),
    Loaded(ThreadState, Vec<RolloutEvent>),
}

#[derive(Debug, Clone)]
pub(super) struct ThreadState {
    pub(super) relative_path: String,
    pub(super) file_size: u64,
    pub(super) line_count: u64,
    pub(super) seq: u64,
    pub(super) generation: Option<u64>,
    pub(super) content: ContentChain,
}

impl ThreadState {
    pub(super) fn empty(relative_path: String) -> Self {
        Self {
            relative_path,
            file_size: 0,
            line_count: 0,
            seq: 0,
            generation: None,
            content: ContentChain::default(),
        }
    }

    pub(super) fn from_events(
        relative_path: String,
        file_size: u64,
        events: &[RolloutEvent],
    ) -> Result<Self, RolloutError> {
        Ok(Self {
            relative_path,
            file_size,
            line_count: events.len() as u64,
            seq: events.last().map_or(0, |event| event.seq),
            generation: last_window_generation(events),
            content: ContentChain::from_events(events)?,
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

impl RolloutStore {
    pub(super) fn resolve_thread_source(
        &self,
        identity_id: &str,
        thread_id: &str,
    ) -> Result<ThreadSource, RolloutError> {
        let key = PathBuf::from(format!("{identity_id}/{thread_id}"));
        if let Some(state) = self.cached_thread(&key) {
            let path = self.checked_absolute_path(&state.relative_path)?;
            if fs::metadata(path).is_ok_and(|metadata| metadata.len() == state.file_size) {
                return Ok(ThreadSource::Cached(state));
            }
        }

        let (state, events) = self.load_thread_source(identity_id, thread_id)?;
        self.cache_thread(key, state.clone());
        Ok(ThreadSource::Loaded(state, events))
    }

    pub(super) fn cache_thread(&self, key: PathBuf, state: ThreadState) {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cache.len() >= MAX_CACHED_THREADS && !cache.contains_key(&key) {
            if let Some(evicted) = cache.keys().next().cloned() {
                cache.remove(&evicted);
            }
        }
        cache.insert(key, state);
    }

    fn cached_thread(&self, key: &Path) -> Option<ThreadState> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
    }

    pub(crate) fn forget_relative_paths(&self, relative_paths: &[String]) {
        let paths = relative_paths
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|_, state| !paths.contains(state.relative_path.as_str()));
    }
}
