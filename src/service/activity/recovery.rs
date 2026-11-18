use chrono::Utc;

use crate::{
    domain::activity::{
        rollout::{RolloutStatus, SessionContext, TurnEndEvent},
        RolloutSummary,
    },
    store::storage::{InterruptedRollout, RecoveredRollout, StorageError},
};

use super::ActivityStore;

impl ActivityStore {
    pub async fn recover_interrupted(&self) -> Result<u64, StorageError> {
        let interrupted = self.storage.list_writing_rollouts().await?;
        let recovered = interrupted.len() as u64;
        for rollout in interrupted {
            let finished = self.finish_interrupted_rollout(&rollout).await?;
            let update = finished
                .map(|summary| RecoveredRollout {
                    activity_id: rollout.activity_id.clone(),
                    status: summary.status,
                    http_status: None,
                    error_code: (summary.status == RolloutStatus::Partial)
                        .then(|| "activity_interrupted".to_string()),
                    error_message: (summary.status == RolloutStatus::Partial).then(|| {
                        "process interrupted before activity rollout completion".to_string()
                    }),
                    event_count: Some(summary.event_count),
                    byte_size: Some(summary.byte_size),
                    sha256: Some(summary.sha256),
                })
                .unwrap_or_else(|| self.recovered_rollout_status(&rollout));
            self.storage.recover_rollout_status(update).await?;
        }
        Ok(recovered)
    }

    async fn finish_interrupted_rollout(
        &self,
        rollout: &InterruptedRollout,
    ) -> Result<Option<RolloutSummary>, StorageError> {
        if rollout.thread_id.is_empty() {
            return Ok(None);
        }
        let session = SessionContext::new(rollout.thread_id.clone());
        let turn = match self.rollouts.find_activity_turn(
            &rollout.identity_id,
            &rollout.thread_id,
            &rollout.activity_id,
        ) {
            Ok(turn) => turn,
            Err(error) => {
                tracing::warn!(
                    activity_id = %rollout.activity_id,
                    error = %error,
                    "could not inspect interrupted activity turn"
                );
                return Ok(None);
            }
        };
        let Some(turn) = turn else {
            return Ok(None);
        };
        if turn.complete {
            return Ok(Some(turn.summary(rollout.activity_id.clone())));
        }
        let mut writer = self
            .rollouts
            .resume_activity_turn(&rollout.identity_id, &session, &rollout.activity_id)
            .await
            .map_err(|error| StorageError::Rollout(error.to_string()))?;
        let summary = writer
            .finish_with_at(
                RolloutStatus::Partial,
                None,
                Some("activity_interrupted".to_string()),
                Some("process interrupted before activity rollout completion".to_string()),
                &Utc::now().to_rfc3339(),
            )
            .map_err(|error| StorageError::Rollout(error.to_string()))?;
        Ok(Some(summary))
    }

    fn recovered_rollout_status(&self, rollout: &InterruptedRollout) -> RecoveredRollout {
        match self
            .rollouts
            .read_slice(&rollout.rollout_path, rollout.start_line, rollout.end_line)
        {
            Ok(file) => {
                let turn_end = file
                    .events
                    .iter()
                    .rev()
                    .find(|event| event.event_type == "turn_end")
                    .and_then(|event| {
                        serde_json::from_value::<TurnEndEvent>(event.payload.clone()).ok()
                    });
                match turn_end {
                    Some(turn_end) => RecoveredRollout {
                        activity_id: rollout.activity_id.clone(),
                        status: turn_end.status,
                        http_status: turn_end.http_status.map(i64::from),
                        error_code: turn_end.error_code,
                        error_message: None,
                        event_count: Some(file.event_count),
                        byte_size: Some(file.byte_size),
                        sha256: Some(file.sha256),
                    },
                    None => RecoveredRollout {
                        activity_id: rollout.activity_id.clone(),
                        status: RolloutStatus::Partial,
                        http_status: None,
                        error_code: Some("activity_interrupted".to_string()),
                        error_message: Some(
                            "process interrupted before activity rollout completion".to_string(),
                        ),
                        event_count: Some(file.event_count),
                        byte_size: Some(file.byte_size),
                        sha256: Some(file.sha256),
                    },
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
