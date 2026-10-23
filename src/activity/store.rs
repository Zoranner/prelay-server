use chrono::{Duration, Utc};
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    activity::{
        rollout::{RolloutStatus, RolloutStore, RolloutWriter, SessionMetaEvent},
        ErrorEvent, RolloutEvent, RolloutEventBody,
    },
    stats::ActivityInsert,
    storage::{InterruptedRollout, RecoveredRollout, ResponseSessionInsert, Storage, StorageError},
};

#[derive(Clone)]
pub struct ActivityStore {
    storage: Storage,
    rollouts: RolloutStore,
}

impl ActivityStore {
    pub fn new(storage: Storage, rollouts: RolloutStore) -> Self {
        Self { storage, rollouts }
    }

    pub fn validate(&self) -> Result<(), StorageError> {
        self.rollouts
            .validate()
            .map_err(|error| StorageError::Rollout(error.to_string()))
    }

    pub fn rollouts(&self) -> &RolloutStore {
        &self.rollouts
    }

    pub async fn recover_interrupted(&self) -> Result<u64, StorageError> {
        let interrupted = self.storage.list_writing_rollouts().await?;
        let recovered = interrupted.len() as u64;
        for rollout in interrupted {
            let update = self.recovered_rollout_status(&rollout);
            self.storage.recover_rollout_status(update).await?;
        }
        Ok(recovered)
    }

    pub async fn orphan_rollout_files(&self) -> Result<Vec<String>, StorageError> {
        let known = self
            .storage
            .list_rollout_paths()
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        let files = self
            .rollouts
            .rollout_files()
            .map_err(|error| StorageError::Rollout(error.to_string()))?;
        Ok(files
            .into_iter()
            .filter(|path| !known.contains(path))
            .collect())
    }

    pub async fn delete_inactive_identities(
        &self,
        now: chrono::DateTime<Utc>,
        retention: Duration,
    ) -> Result<u64, StorageError> {
        let result = self
            .storage
            .delete_inactive_identities(now, retention)
            .await?;
        self.remove_rollout_files(&result.rollout_paths).await;
        Ok(result.deleted)
    }

    pub async fn record_completed(
        &self,
        identity_id: &str,
        activity: ActivityInsert,
        session_id: Option<&str>,
        request_events: Vec<RolloutEventBody>,
        response_events: Vec<RolloutEventBody>,
    ) -> Result<(), StorageError> {
        let activity_id = Uuid::new_v4().to_string();
        let mut writer = self.begin_rollout(
            &activity_id,
            identity_id,
            &activity,
            session_id,
            request_events,
        )?;
        for event in response_events {
            writer
                .append(event)
                .map_err(|error| StorageError::Rollout(error.to_string()))?;
        }
        if let Some(message) = activity.error_message.as_deref() {
            writer
                .append(RolloutEventBody::Error(ErrorEvent {
                    code: activity.error_code.clone(),
                    message: message.to_string(),
                    http_status: u16::try_from(activity.http_status).ok(),
                }))
                .map_err(|error| StorageError::Rollout(error.to_string()))?;
        }
        let status = if activity.status == "failed" {
            RolloutStatus::Failed
        } else {
            RolloutStatus::Completed
        };
        let summary = writer
            .finish_with(
                status,
                u16::try_from(activity.http_status).ok(),
                activity.error_code.clone(),
                activity.error_message.clone(),
            )
            .map_err(|error| StorageError::Rollout(error.to_string()))?;
        self.storage
            .insert_activity_with_rollout(identity_id, activity_id, activity, summary)
            .await
    }

    pub fn begin_stream(
        &self,
        identity_id: &str,
        activity: &ActivityInsert,
        session_id: Option<&str>,
        request_events: Vec<RolloutEventBody>,
    ) -> Result<(String, RolloutWriter), StorageError> {
        let activity_id = Uuid::new_v4().to_string();
        let writer = self.begin_rollout(
            &activity_id,
            identity_id,
            activity,
            session_id,
            request_events,
        )?;
        Ok((activity_id, writer))
    }

    pub async fn start_stream(
        &self,
        identity_id: &str,
        activity_id: &str,
        activity: ActivityInsert,
        summary: crate::activity::rollout::RolloutSummary,
    ) -> Result<(), StorageError> {
        self.storage
            .start_stream_activity_with_rollout(identity_id, activity_id, activity, summary)
            .await
    }

    pub async fn insert_stream_result(
        &self,
        identity_id: &str,
        activity_id: String,
        activity: ActivityInsert,
        summary: crate::activity::rollout::RolloutSummary,
    ) -> Result<(), StorageError> {
        self.storage
            .insert_activity_with_rollout(identity_id, activity_id, activity, summary)
            .await
    }

    pub async fn complete_stream(
        &self,
        activity_id: &str,
        summary: crate::activity::rollout::RolloutSummary,
    ) -> Result<(), StorageError> {
        self.storage
            .complete_activity_rollout(activity_id, summary)
            .await
    }

    pub async fn update_stream_activity(
        &self,
        identity_id: &str,
        activity_id: &str,
        update: crate::stats::StreamActivityUpdate,
    ) -> Result<(), StorageError> {
        self.storage
            .update_stream_activity(identity_id, activity_id, update)
            .await
    }

    pub async fn save_response_session(
        &self,
        insert: ResponseSessionInsert<'_>,
    ) -> Result<(), StorageError> {
        self.storage.save_response_session(insert).await
    }

    fn begin_rollout(
        &self,
        activity_id: &str,
        identity_id: &str,
        activity: &ActivityInsert,
        session_id: Option<&str>,
        request_events: Vec<RolloutEventBody>,
    ) -> Result<RolloutWriter, StorageError> {
        let session_id = session_id
            .filter(|value| !value.is_empty())
            .unwrap_or(activity_id)
            .to_string();
        let meta = SessionMetaEvent {
            activity_id: activity_id.to_string(),
            session_id,
            identity_id: identity_id.to_string(),
            protocol_in: activity.protocol_in.clone(),
            protocol_out: activity.protocol_out.clone(),
            protocol_upstream: Some(activity.protocol_upstream.clone()),
            endpoint_name: activity.endpoint_name.clone(),
            model_requested: activity.model_requested.clone(),
            model_upstream: activity.model_upstream.clone(),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        self.rollouts
            .begin(meta, request_events)
            .map_err(|error| StorageError::Rollout(error.to_string()))
    }

    async fn remove_rollout_files(&self, relative_paths: &[String]) {
        for relative_path in relative_paths {
            let path = match self.rollouts.checked_absolute_path(relative_path) {
                Ok(path) => path,
                Err(error) => {
                    tracing::warn!(
                        rollout_path = %relative_path,
                        error = %error,
                        "refused to remove a rollout file outside the configured root"
                    );
                    continue;
                }
            };
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tracing::warn!(
                        rollout_path = %path.display(),
                        "rollout file was already missing during identity cleanup"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        rollout_path = %path.display(),
                        error = %error,
                        "failed to remove rollout file during identity cleanup"
                    );
                }
            }
        }
    }

    fn recovered_rollout_status(&self, rollout: &InterruptedRollout) -> RecoveredRollout {
        match self.rollouts.read_verified(&rollout.rollout_path) {
            Ok(file) => {
                let (status, http_status, error_code, error_message) =
                    terminal_session_end(&file.events).unwrap_or_else(|| {
                        (
                            RolloutStatus::Partial,
                            None,
                            Some("activity_interrupted".to_string()),
                            Some(
                                "process interrupted before activity rollout completion"
                                    .to_string(),
                            ),
                        )
                    });
                RecoveredRollout {
                    activity_id: rollout.activity_id.clone(),
                    status,
                    http_status,
                    error_code,
                    error_message,
                    event_count: Some(file.event_count),
                    byte_size: Some(file.byte_size),
                    sha256: Some(file.sha256),
                }
            }
            Err(error) => {
                tracing::warn!(
                    activity_id = %rollout.activity_id,
                    rollout_path = %rollout.rollout_path,
                    error = %error,
                    "could not verify interrupted rollout; marking it partial"
                );
                RecoveredRollout {
                    activity_id: rollout.activity_id.clone(),
                    status: RolloutStatus::Partial,
                    http_status: None,
                    error_code: Some("activity_interrupted".to_string()),
                    error_message: Some(
                        "process interrupted before activity rollout completion".to_string(),
                    ),
                    event_count: None,
                    byte_size: None,
                    sha256: None,
                }
            }
        }
    }
}

type TerminalRollout = (RolloutStatus, Option<i64>, Option<String>, Option<String>);

fn terminal_session_end(events: &[RolloutEvent]) -> Option<TerminalRollout> {
    let event = events
        .last()
        .filter(|event| event.event_type == "session_end")?;
    let status = match event.payload.get("status")?.as_str()? {
        "completed" => RolloutStatus::Completed,
        "failed" => RolloutStatus::Failed,
        "partial" => RolloutStatus::Partial,
        _ => return None,
    };
    let http_status = event
        .payload
        .get("http_status")
        .and_then(serde_json::Value::as_i64);
    let error_code = event
        .payload
        .get("error_code")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let error_message = event
        .payload
        .get("error_message")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Some((status, http_status, error_code, error_message))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::terminal_session_end;
    use crate::activity::rollout::{RolloutEvent, RolloutStatus};

    fn event(event_type: &str, payload: serde_json::Value) -> RolloutEvent {
        RolloutEvent {
            seq: 1,
            timestamp: "2026-10-23T00:00:00Z".to_string(),
            event_type: event_type.to_string(),
            payload,
        }
    }

    #[test]
    fn maps_a_completed_session_end_to_completed_status() {
        let events = vec![event(
            "session_end",
            json!({ "status": "completed", "http_status": 200 }),
        )];

        assert_eq!(
            terminal_session_end(&events),
            Some((RolloutStatus::Completed, Some(200), None, None))
        );
    }

    #[test]
    fn maps_a_failed_session_end_and_ignores_non_terminal_files() {
        let events = vec![event(
            "session_end",
            json!({
                "status": "failed",
                "http_status": 502,
                "error_code": "stream_error",
                "error_message": "upstream failed"
            }),
        )];

        assert_eq!(
            terminal_session_end(&events),
            Some((
                RolloutStatus::Failed,
                Some(502),
                Some("stream_error".to_string()),
                Some("upstream failed".to_string())
            ))
        );
        assert_eq!(
            terminal_session_end(&[event("message", json!({ "content": "partial" }))]),
            None
        );
    }
}
