use super::{
    read::ThreadEvents, ActivityTurn, CompletedTurnMaterialization, FinishedTurnMaterialization,
    RolloutStore, StartedTurnMaterialization,
};
use crate::domain::activity::rollout::{
    ConversationEvent, RolloutError, RolloutStatus, RolloutSummary, TurnEndEvent, WindowEvent,
};

/// 结束某个 turn 的 `turn_end` 及其在文件中的行号。
pub(super) struct TurnCompletion {
    pub(super) end: TurnEndEvent,
    pub(super) line: u64,
}

/// 找到属于指定 turn 的 `turn_end`。
///
/// 新事件在 `turn_end` 里带上 `turn_id`，可以精确归属；旧格式没有该字段时，
/// 只能退回到“出现在该 turn 与下一个 turn 之间”的位置判断。
pub(super) fn turn_completion(
    thread: &ThreadEvents,
    turn_index: usize,
) -> Result<Option<TurnCompletion>, RolloutError> {
    let Some((index, end)) = completion_index(&thread.events, turn_index)? else {
        return Ok(None);
    };
    let line = thread.lines.get(index).copied().ok_or_else(|| {
        RolloutError::InvalidPath("turn_end event has no file position".to_string())
    })?;
    Ok(Some(TurnCompletion { end, line }))
}

/// 按 `activity_id` 找到结束该活动的 `turn_end`。
///
/// 供恢复路径使用：只看保存下来的事件切片时，同样不能用“切片里最后一个 `turn_end`”。
pub(crate) fn activity_turn_end(
    events: &[ConversationEvent],
    activity_id: &str,
) -> Option<TurnEndEvent> {
    let turn_index = events.iter().position(|event| {
        event.event_type == "turn"
            && event
                .payload
                .get("activity_id")
                .and_then(serde_json::Value::as_str)
                == Some(activity_id)
    })?;
    completion_index(events, turn_index)
        .ok()
        .flatten()
        .map(|(_, end)| end)
}

/// 返回 `(事件下标, turn_end)`。
///
/// 先按 `turn_id` 归属再解析：属于别的 turn 的坏记录不能让本次查询失败。
fn completion_index(
    events: &[ConversationEvent],
    turn_index: usize,
) -> Result<Option<(usize, TurnEndEvent)>, RolloutError> {
    let turn_id = events[turn_index]
        .payload
        .get("turn_id")
        .and_then(serde_json::Value::as_str);
    let next_turn_index = events
        .iter()
        .enumerate()
        .skip(turn_index + 1)
        .find(|(_, event)| event.event_type == "turn")
        .map(|(index, _)| index);
    for (index, event) in events.iter().enumerate().skip(turn_index + 1) {
        if event.event_type != "turn_end" {
            continue;
        }
        let end_turn_id = event
            .payload
            .get("turn_id")
            .and_then(serde_json::Value::as_str);
        let owns_turn = match (end_turn_id, turn_id) {
            (Some(owner), Some(turn_id)) => owner == turn_id,
            (Some(_), None) => false,
            (None, _) => next_turn_index.is_none_or(|next| index < next),
        };
        if !owns_turn {
            continue;
        }
        let end: TurnEndEvent = serde_json::from_value(event.payload.clone())?;
        return Ok(Some((index, end)));
    }
    Ok(None)
}

impl RolloutStore {
    pub(crate) fn find_activity_turn(
        &self,
        identity_id: &str,
        thread_id: &str,
        activity_id: &str,
    ) -> Result<Option<ActivityTurn>, RolloutError> {
        let (state, thread) = self.load_thread_source(identity_id, thread_id)?;
        let Some(turn_index) = thread.events.iter().position(|event| {
            event.event_type == "turn"
                && event
                    .payload
                    .get("activity_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(activity_id)
        }) else {
            return Ok(None);
        };
        let next_turn_index = thread
            .events
            .iter()
            .enumerate()
            .skip(turn_index + 1)
            .find(|(_, event)| event.event_type == "turn")
            .map(|(index, _)| index);
        // 首个 turn 的持久化区间从文件开头算起（包含文件头与窗口行），
        // 后续 turn 从自身所在行开始。
        let previous_turn = thread.events[..turn_index]
            .iter()
            .rposition(|event| event.event_type == "turn");
        let start_line = if previous_turn.is_some() {
            thread.lines.get(turn_index).copied().ok_or_else(|| {
                RolloutError::InvalidPath("turn event has no file position".to_string())
            })?
        } else {
            1
        };
        let positional_end = next_turn_index
            .and_then(|index| thread.lines.get(index).copied())
            .map_or(state.line_count, |line| line.saturating_sub(1));
        let completion = turn_completion(&thread, turn_index)?;
        let end_line = completion.as_ref().map_or(positional_end, |completion| {
            positional_end.max(completion.line)
        });
        let slice = self.read_slice(&state.relative_path, start_line, end_line)?;
        let mut window_id = None;
        let mut generation = 0;
        for event in &thread.events[..=turn_index] {
            if event.event_type != "window" {
                continue;
            }
            if let Ok(window) = serde_json::from_value::<WindowEvent>(event.payload.clone()) {
                window_id = window.window_id;
                generation = window.generation;
            }
        }
        let status = completion
            .as_ref()
            .map_or(RolloutStatus::Writing, |completion| completion.end.status);
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
            complete: completion.is_some(),
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
