use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::Arc,
};

use sha2::{Digest, Sha256};

use super::{super::support::is_safe_path_segment, RolloutWriter};
use crate::{
    domain::activity::rollout::{
        ConversationFormat, RolloutError, RolloutStatus, SessionContext, TurnEvent, WindowEvent,
    },
    store::rollout::RolloutStore,
};

impl RolloutStore {
    pub(crate) async fn resume_activity_turn(
        &self,
        identity_id: &str,
        session: &SessionContext,
        activity_id: &str,
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
        let (state, thread) = self.load_thread_source(identity_id, &session.thread_id)?;
        let events = &thread.events;
        let turn_index = events
            .iter()
            .position(|event| {
                event.event_type == "turn"
                    && event
                        .payload
                        .get("activity_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(activity_id)
            })
            .ok_or_else(|| {
                RolloutError::InvalidPath(format!("activity turn {activity_id} does not exist"))
            })?;
        if events[turn_index + 1..]
            .iter()
            .any(|event| event.event_type == "turn")
        {
            return Err(RolloutError::InvalidPath(format!(
                "activity turn {activity_id} is not the last turn"
            )));
        }
        if super::super::activity::turn_completion(&thread, turn_index)?.is_some() {
            return Err(RolloutError::InvalidPath(format!(
                "activity turn {activity_id} is already complete"
            )));
        }
        let turn: TurnEvent = serde_json::from_value(events[turn_index].payload.clone())
            .map_err(|error| RolloutError::InvalidPath(format!("invalid turn event: {error}")))?;
        let mut window_id = session.window_id.clone();
        let mut generation = session.generation;
        for event in &events[..=turn_index] {
            if event.event_type != "window" {
                continue;
            }
            if let Ok(window) = serde_json::from_value::<WindowEvent>(event.payload.clone()) {
                window_id = window.window_id;
                generation = window.generation;
            }
        }
        let meta_lines = u64::from(state.format == ConversationFormat::V1);
        let previous_turn = events[..turn_index]
            .iter()
            .rposition(|event| event.event_type == "turn");
        let start_line = if previous_turn.is_some() {
            meta_lines + turn_index as u64 + 1
        } else {
            1
        };
        let final_path = self.checked_absolute_path(&state.relative_path)?;
        let file = OpenOptions::new().append(true).open(&final_path)?;
        let thread_relative = format!("{identity_id}/{}.jsonl", session.thread_id);
        let thread_path = self.checked_absolute_path(&thread_relative)?;
        let index_path = self.checked_absolute_path(&format!("{thread_relative}.index"))?;
        Ok(RolloutWriter {
            activity_id: activity_id.to_string(),
            thread_id: session.thread_id.clone(),
            window_id,
            generation,
            format: state.format,
            turn_id: turn.turn_id,
            relative_path: state.relative_path,
            final_path,
            thread_relative,
            thread_path,
            index_path,
            file: Some(BufWriter::new(file)),
            _lock: Some(guard),
            hasher: Sha256::new(),
            seq: state.seq,
            line_count: state.line_count,
            start_line,
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
        })
    }
}

impl RolloutWriter {
    pub(crate) fn sync_progress(&mut self) -> Result<(), RolloutError> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        file.flush()?;
        file.get_ref().sync_all()?;
        Ok(())
    }
}
