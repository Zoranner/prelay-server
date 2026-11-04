use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chrono::{FixedOffset, SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

use super::{
    cache::{FileStamp, ThreadIndex, ThreadSource, ThreadState, WindowState},
    support::{
        draft_event_parts, event_identity, event_parts, is_content_event, is_safe_path_segment,
        item_id, item_ref_key, ContentHash,
    },
    RolloutStore,
};
use crate::domain::activity::rollout::{
    ContentDraft, ContextEvent, ConversationEvent, ConversationEventBody, ConversationFormat,
    ConversationMeta, RolloutError, RolloutStatus, RolloutSummary, SessionContext, TurnContext,
    TurnEndEvent, TurnEvent, WindowEvent,
};

pub struct RolloutWriter {
    activity_id: String,
    thread_id: String,
    window_id: Option<String>,
    generation: u64,
    format: ConversationFormat,
    turn_id: String,
    relative_path: String,
    pub(super) final_path: PathBuf,
    pub(super) thread_relative: String,
    pub(super) thread_path: PathBuf,
    pub(super) index_path: PathBuf,
    file: Option<BufWriter<std::fs::File>>,
    _lock: Option<OwnedMutexGuard<()>>,
    hasher: Sha256,
    seq: u64,
    line_count: u64,
    start_line: u64,
    end_line: u64,
    byte_size: u64,
    expected_len: u64,
    event_count: u64,
    content: ContentHash,
    event_refs: HashMap<[u8; 32], Vec<u64>>,
    item_refs: HashMap<String, u64>,
    content_event_count: u64,
    window_state: Option<WindowState>,
    cache_key: PathBuf,
    cache: Arc<Mutex<ThreadIndex>>,
    status: RolloutStatus,
    finished: bool,
}

impl RolloutStore {
    pub async fn begin(
        &self,
        identity_id: &str,
        session: &SessionContext,
        turn: TurnContext,
        request_events: Vec<ContentDraft>,
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

        let cache_key = PathBuf::from(format!("{identity_id}/{}", session.thread_id));
        let lock = self.lock_for(&cache_key);
        let guard = lock.lock_owned().await;
        let store = self.clone();
        let identity = identity_id.to_string();
        let thread = session.thread_id.clone();
        let source =
            tokio::task::spawn_blocking(move || store.resolve_thread_source(&identity, &thread))
                .await
                .map_err(|error| {
                    RolloutError::Io(io::Error::other(format!(
                        "rollout read task failed: {error}"
                    )))
                })??;
        let state = match source {
            ThreadSource::Cached(state) => state,
            ThreadSource::Loaded(state, _events) => state,
        };
        let window_changed = state.window_changed(session);
        let needs_window = state.window.is_none() || window_changed;
        let is_empty = state.is_empty();
        let format = if is_empty {
            ConversationFormat::V1
        } else {
            state.format
        };
        let relative_path = state.relative_path.clone();
        let final_path = self.checked_absolute_path(&relative_path)?;
        let parent = final_path
            .parent()
            .ok_or_else(|| RolloutError::InvalidPath(relative_path.clone()))?;
        fs::create_dir_all(parent)?;
        let thread_relative = format!("{identity_id}/{}.jsonl", session.thread_id);
        let thread_path = self.checked_absolute_path(&thread_relative)?;
        let index_relative = format!("{thread_relative}.index");
        let index_path = self.checked_absolute_path(&index_relative)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&final_path)?;
        let mut writer = RolloutWriter {
            activity_id: turn.activity_id,
            thread_id: session.thread_id.clone(),
            window_id: session.window_id.clone(),
            generation: session.generation,
            format,
            turn_id: format!("turn_{}", Uuid::new_v4().simple()),
            relative_path,
            final_path,
            thread_relative,
            thread_path,
            index_path,
            file: Some(BufWriter::new(file)),
            _lock: Some(guard),
            hasher: Sha256::new(),
            seq: state.seq,
            line_count: state.line_count,
            start_line: state.line_count + 1,
            end_line: state.line_count,
            byte_size: 0,
            expected_len: state.stamp.len,
            event_count: 0,
            content: state.content,
            event_refs: state.event_refs,
            item_refs: state.item_refs,
            content_event_count: state.content_event_count,
            window_state: state.window,
            cache_key,
            cache: Arc::clone(&self.cache),
            status: RolloutStatus::Writing,
            finished: false,
        };

        if is_empty {
            writer.write_meta()?;
        }
        if is_empty || needs_window {
            writer.append_control(ConversationEventBody::Window(WindowEvent {
                window_id: session.window_id.clone(),
                generation: session.generation,
            }))?;
        }
        writer.append_control(ConversationEventBody::Turn(TurnEvent {
            turn_id: writer.turn_id.clone(),
            activity_id: writer.activity_id.clone(),
            model: turn.model_requested,
            upstream_model: turn.model_upstream,
        }))?;
        writer.append_request_events(request_events)?;
        Ok(writer)
    }
}

impl RolloutWriter {
    pub fn append_content(&mut self, draft: ContentDraft) -> Result<u64, RolloutError> {
        let (event_type, payload) = draft_event_parts(&draft)?;
        let item_key = item_id(&draft.body)
            .or_else(|| draft.item_id.clone())
            .map(|id| item_ref_key(&event_type, &id));
        let seq = self.write_event(event_type, payload, true)?;
        if let Some(item_key) = item_key {
            self.item_refs.insert(item_key, seq);
        }
        Ok(seq)
    }

    pub fn finish(&mut self, status: RolloutStatus) -> Result<RolloutSummary, RolloutError> {
        self.finish_with(status, None, None, None)
    }

    pub fn finish_with(
        &mut self,
        status: RolloutStatus,
        http_status: Option<u16>,
        error_code: Option<String>,
        _error_message: Option<String>,
    ) -> Result<RolloutSummary, RolloutError> {
        if self.finished {
            return Ok(self.summary_with_status(self.status));
        }
        self.append_control(ConversationEventBody::TurnEnd(TurnEndEvent {
            status,
            http_status,
            error_code,
        }))?;
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            file.get_ref().sync_all()?;
        }
        let expected_len = self.expected_len + self.byte_size;
        match fs::metadata(&self.final_path) {
            Ok(metadata) if metadata.len() != expected_len => {
                tracing::warn!(
                    rollout_path = %self.final_path.display(),
                    expected_len,
                    actual_len = metadata.len(),
                    "rollout file length changed outside the writer; skipping cache update"
                );
            }
            Ok(metadata) => {
                let state = ThreadState {
                    relative_path: self.relative_path.clone(),
                    format: self.format,
                    stamp: FileStamp::from_metadata(&metadata),
                    line_count: self.line_count,
                    seq: self.seq,
                    window: self.window_state.clone(),
                    content: self.content.clone(),
                    event_refs: self.event_refs.clone(),
                    item_refs: self.item_refs.clone(),
                    content_event_count: self.content_event_count,
                };
                self.cache
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(self.cache_key.clone(), state.clone());
                self.persist_index(&state);
            }
            Err(error) => {
                tracing::warn!(
                    rollout_path = %self.final_path.display(),
                    error = %error,
                    "failed to read rollout metadata after finish; skipping cache update"
                );
            }
        }
        self._lock.take();
        self.status = status;
        self.finished = true;
        Ok(self.summary_with_status(self.status))
    }

    pub fn summary(&self) -> RolloutSummary {
        self.summary_with_status(self.status)
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn final_path(&self) -> &Path {
        &self.final_path
    }

    fn append_request_events(&mut self, drafts: Vec<ContentDraft>) -> Result<(), RolloutError> {
        let mut refs = Vec::with_capacity(drafts.len());
        let mut occurrences = HashMap::<[u8; 32], usize>::new();
        for draft in drafts {
            let (event_type, payload) = draft_event_parts(&draft)?;
            let identity = event_identity(&event_type, &payload)?;
            let occurrence = occurrences.entry(identity).or_default();
            let stable_id = item_id(&draft.body).or_else(|| draft.item_id.clone());
            let item_key = stable_id.as_ref().map(|id| item_ref_key(&event_type, id));
            let existing = item_key
                .as_ref()
                .and_then(|key| self.item_refs.get(key).copied())
                .or_else(|| {
                    self.event_refs
                        .get(&identity)
                        .and_then(|refs| refs.get(*occurrence).copied())
                });
            *occurrence += 1;
            let seq = match existing {
                Some(seq) => seq,
                None => self.write_event(event_type, payload, true)?,
            };
            if let Some(item_key) = item_key {
                self.item_refs.insert(item_key, seq);
            }
            refs.push(seq);
        }
        self.append_control(ConversationEventBody::Context(ContextEvent { refs }))
    }

    fn append_control(&mut self, body: ConversationEventBody) -> Result<(), RolloutError> {
        let (event_type, payload) = event_parts(&body)?;
        if event_type == "window" {
            self.window_state = serde_json::from_value::<WindowEvent>(payload.clone())
                .ok()
                .map(|window| WindowState {
                    window_id: window.window_id,
                    generation: window.generation,
                });
            self.event_refs.clear();
        }
        self.write_event(event_type, payload, false).map(|_| ())
    }

    fn write_event(
        &mut self,
        event_type: String,
        payload: serde_json::Value,
        content: bool,
    ) -> Result<u64, RolloutError> {
        if self.finished {
            return Err(RolloutError::InvalidPath(
                "rollout is already finished".to_string(),
            ));
        }
        if content {
            self.content.append(&event_type, &payload)?;
        }
        self.seq += 1;
        let event = ConversationEvent {
            seq: self.seq,
            time: readable_time(),
            event_type,
            payload,
        };
        let mut line = serde_json::to_vec(&event)?;
        line.push(b'\n');
        self.write_line(&line)?;
        self.event_count += 1;
        if content && is_content_event(&event.event_type) {
            let identity = event_identity(&event.event_type, &event.payload)?;
            self.event_refs.entry(identity).or_default().push(event.seq);
            self.content_event_count += 1;
        }
        Ok(event.seq)
    }

    fn write_meta(&mut self) -> Result<(), RolloutError> {
        let meta = ConversationMeta::new(self.thread_id.clone(), readable_time());
        let mut line = serde_json::to_vec(&meta)?;
        line.push(b'\n');
        self.write_line(&line)
    }

    fn write_line(&mut self, line: &[u8]) -> Result<(), RolloutError> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| RolloutError::InvalidPath("rollout file is closed".to_string()))?;
        file.write_all(line)?;
        file.flush()?;
        self.hasher.update(line);
        self.byte_size += line.len() as u64;
        self.line_count += 1;
        self.end_line = self.line_count;
        Ok(())
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

fn readable_time() -> String {
    let offset = FixedOffset::east_opt(8 * 60 * 60).expect("valid +08:00 offset");
    Utc::now()
        .with_timezone(&offset)
        .to_rfc3339_opts(SecondsFormat::Millis, false)
}
