use super::{
    ActivityTurn, CompletedTurnMaterialization, FinishedTurnMaterialization, RolloutStore,
    StartedTurnMaterialization,
};
use crate::domain::activity::rollout::{
    ConversationFormat, RolloutError, RolloutStatus, RolloutSummary, TurnEndEvent, WindowEvent,
};

impl RolloutStore {
    pub(crate) fn find_activity_turn(
        &self,
        identity_id: &str,
        thread_id: &str,
        activity_id: &str,
    ) -> Result<Option<ActivityTurn>, RolloutError> {
        let (state, events) = self.load_thread_source(identity_id, thread_id)?;
        let Some(turn_index) = events.iter().position(|event| {
            event.event_type == "turn"
                && event
                    .payload
                    .get("activity_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(activity_id)
        }) else {
            return Ok(None);
        };
        let next_turn_index = events
            .iter()
            .enumerate()
            .skip(turn_index + 1)
            .find(|(_, event)| event.event_type == "turn")
            .map(|(index, _)| index);
        let meta_lines = u64::from(state.format == ConversationFormat::V1);
        let previous_turn = events[..turn_index]
            .iter()
            .rposition(|event| event.event_type == "turn");
        let start_line = if previous_turn.is_some() {
            meta_lines + turn_index as u64 + 1
        } else {
            1
        };
        let end_line = next_turn_index
            .map(|index| meta_lines + index as u64)
            .unwrap_or(state.line_count);
        let slice = self.read_slice(&state.relative_path, start_line, end_line)?;
        let mut window_id = None;
        let mut generation = 0;
        for event in &events[..=turn_index] {
            if event.event_type != "window" {
                continue;
            }
            if let Ok(window) = serde_json::from_value::<WindowEvent>(event.payload.clone()) {
                window_id = window.window_id;
                generation = window.generation;
            }
        }
        let turn_end = slice
            .events
            .iter()
            .rev()
            .find(|event| event.event_type == "turn_end");
        let status = match turn_end {
            Some(event) => serde_json::from_value::<TurnEndEvent>(event.payload.clone())?.status,
            None => RolloutStatus::Writing,
        };
        Ok(Some(ActivityTurn {
            relative_path: state.relative_path,
            thread_id: thread_id.to_string(),
            window_id,
            generation,
            status,
            start_line,
            end_line,
            event_count: slice.event_count,
            byte_size: slice.byte_size,
            sha256: slice.sha256,
            complete: turn_end.is_some(),
        }))
    }

    pub(crate) async fn materialize_completed_turn(
        &self,
        identity_id: &str,
        materialization: CompletedTurnMaterialization,
    ) -> Result<RolloutSummary, RolloutError> {
        let CompletedTurnMaterialization {
            session,
            turn,
            created_at,
            request_events,
            response_events,
            status,
            http_status,
            error_code,
            error_message,
            finished_at,
        } = materialization;
        let activity_id = turn.activity_id.clone();
        if let Some(existing) =
            self.find_activity_turn(identity_id, &session.thread_id, &activity_id)?
        {
            if !existing.complete {
                return Err(RolloutError::InvalidPath(format!(
                    "activity turn {activity_id} is incomplete and requires recovery"
                )));
            }
            return Ok(existing.summary(activity_id));
        }

        let mut writer = self
            .begin_at(identity_id, &session, turn, created_at, request_events)
            .await?;
        for event in response_events {
            writer.append_content_at(event.draft, &event.observed_at)?;
        }
        writer.finish_with_at(status, http_status, error_code, error_message, &finished_at)
    }

    pub(crate) async fn materialize_started_turn(
        &self,
        identity_id: &str,
        materialization: StartedTurnMaterialization,
    ) -> Result<ActivityTurn, RolloutError> {
        let StartedTurnMaterialization {
            session,
            turn,
            created_at,
            request_events,
        } = materialization;
        let activity_id = turn.activity_id.clone();
        if let Some(existing) =
            self.find_activity_turn(identity_id, &session.thread_id, &activity_id)?
        {
            return Ok(existing);
        }
        let mut writer = self
            .begin_at(identity_id, &session, turn, created_at, request_events)
            .await?;
        writer.sync_progress()?;
        drop(writer);
        self.find_activity_turn(identity_id, &session.thread_id, &activity_id)?
            .ok_or_else(|| {
                RolloutError::InvalidPath("started turn was not written to the thread".to_string())
            })
    }

    pub(crate) async fn materialize_finished_turn(
        &self,
        identity_id: &str,
        materialization: FinishedTurnMaterialization,
    ) -> Result<RolloutSummary, RolloutError> {
        let FinishedTurnMaterialization {
            session,
            activity_id,
            response_events,
            status,
            http_status,
            error_code,
            error_message,
            finished_at,
        } = materialization;
        let Some(existing) =
            self.find_activity_turn(identity_id, &session.thread_id, &activity_id)?
        else {
            return Err(RolloutError::InvalidPath(format!(
                "finished turn {activity_id} has no started turn"
            )));
        };
        if existing.complete {
            return Ok(existing.summary(activity_id));
        }
        let mut writer = self
            .resume_activity_turn(identity_id, &session, &activity_id)
            .await?;
        for event in response_events {
            writer.append_content_at(event.draft, &event.observed_at)?;
        }
        writer.finish_with_at(status, http_status, error_code, error_message, &finished_at)?;
        drop(writer);
        let completed = self
            .find_activity_turn(identity_id, &session.thread_id, &activity_id)?
            .ok_or_else(|| {
                RolloutError::InvalidPath("finished turn was not written to the thread".to_string())
            })?;
        if !completed.complete {
            return Err(RolloutError::InvalidPath(
                "finished turn is missing turn_end".to_string(),
            ));
        }
        Ok(completed.summary(activity_id))
    }
}
