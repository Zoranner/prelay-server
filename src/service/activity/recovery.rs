use chrono::Utc;

use crate::{
    domain::activity::{
        rollout::{RolloutStatus, SessionContext},
        ActivityErrorCode, RolloutSummary,
    },
    store::storage::{InterruptedRollout, RecoveredRollout, StorageError},
};

use super::ActivityStore;

const INTERRUPTED_CODE: &str = ActivityErrorCode::ACTIVITY_INTERRUPTED;
const INTERRUPTED_MESSAGE: &str = "process interrupted before activity rollout completion";

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
                        .then(|| INTERRUPTED_CODE.to_string()),
                    error_message: (summary.status == RolloutStatus::Partial)
                        .then(|| INTERRUPTED_MESSAGE.to_string()),
                    start_line: Some(summary.start_line),
                    end_line: Some(summary.end_line),
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
        self.rollouts
            .finish_activity_turn(
                &rollout.identity_id,
                crate::store::rollout::FinishedTurnMaterialization {
                    session,
                    activity_id: rollout.activity_id.clone(),
                    response_events: Vec::new(),
                    status: RolloutStatus::Partial,
                    http_status: None,
                    error_code: Some(INTERRUPTED_CODE.to_string()),
                    error_message: Some(INTERRUPTED_MESSAGE.to_string()),
                    finished_at: Utc::now().to_rfc3339(),
                },
            )
            .await
            .map(Some)
            .map_err(|error| StorageError::Rollout(error.to_string()))
    }

    fn recovered_rollout_status(&self, rollout: &InterruptedRollout) -> RecoveredRollout {
        match self
            .rollouts
            .read_slice(&rollout.rollout_path, rollout.start_line, rollout.end_line)
        {
            Ok(file) => {
                let turn_end =
                    crate::store::rollout::activity_turn_end(&file.events, &rollout.activity_id);
                match turn_end {
                    Some(turn_end) => RecoveredRollout {
                        activity_id: rollout.activity_id.clone(),
                        status: turn_end.status,
                        http_status: turn_end.http_status.map(i64::from),
                        error_code: turn_end.error_code,
                        error_message: None,
                        start_line: Some(rollout.start_line),
                        end_line: Some(rollout.end_line),
                        event_count: Some(file.event_count),
                        byte_size: Some(file.byte_size),
                        sha256: Some(file.sha256),
                    },
                    None => RecoveredRollout {
                        activity_id: rollout.activity_id.clone(),
                        status: RolloutStatus::Partial,
                        http_status: None,
                        error_code: Some(INTERRUPTED_CODE.to_string()),
                        error_message: Some(INTERRUPTED_MESSAGE.to_string()),
                        start_line: Some(rollout.start_line),
                        end_line: Some(rollout.end_line),
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
                    error_code: Some(INTERRUPTED_CODE.to_string()),
                    error_message: Some(INTERRUPTED_MESSAGE.to_string()),
                    start_line: Some(rollout.start_line),
                    end_line: Some(rollout.end_line),
                    event_count: None,
                    byte_size: None,
                    sha256: None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::activity::rollout::{SessionContext, TurnContext};
    use crate::domain::activity::ActivityErrorCode;
    use crate::domain::stats::ActivityInsert;
    use crate::service::activity::ActivityStore;
    use crate::store::rollout::RolloutStore;

    #[tokio::test]
    async fn recovers_interleaved_writing_activity_with_full_envelope_range() {
        let storage = crate::test_support::test_storage().await;
        let identity = storage
            .register_identity(
                "recover-interleaved-machine",
                "S-recover-interleaved",
                &crate::domain::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity");
        let root = tempfile::tempdir().expect("temp rollout root");
        let rollouts = RolloutStore::new(root.path().to_path_buf());
        let session = SessionContext::new("thread-interleaved-recovery");
        let first = rollouts
            .begin(
                &identity.identity_id,
                &session,
                TurnContext {
                    activity_id: "interleaved-a".to_string(),
                    model_requested: "deepseek-flash".to_string(),
                    model_upstream: "deepseek-chat".to_string(),
                },
                Vec::new(),
            )
            .await
            .expect("begin earlier turn");
        drop(first);
        let later = rollouts
            .begin(
                &identity.identity_id,
                &session,
                TurnContext {
                    activity_id: "interleaved-b".to_string(),
                    model_requested: "deepseek-flash".to_string(),
                    model_upstream: "deepseek-chat".to_string(),
                },
                Vec::new(),
            )
            .await
            .expect("begin later turn");
        drop(later);

        let started = rollouts
            .find_activity_turn(
                &identity.identity_id,
                "thread-interleaved-recovery",
                "interleaved-a",
            )
            .expect("find started turn")
            .expect("started turn exists");
        assert!(!started.complete);
        let mut activity = test_activity();
        activity.status = "streaming".to_string();
        activity.is_streaming = true;
        storage
            .insert_activity_with_rollout(
                &identity.identity_id,
                "interleaved-a".to_string(),
                activity,
                started.summary("interleaved-a"),
            )
            .await
            .expect("insert interrupted activity");

        let activity_store = ActivityStore::new(storage.clone(), rollouts.clone());
        assert_eq!(
            activity_store
                .recover_interrupted()
                .await
                .expect("recover interrupted activity"),
            1
        );

        let completed = rollouts
            .find_activity_turn(
                &identity.identity_id,
                "thread-interleaved-recovery",
                "interleaved-a",
            )
            .expect("find recovered turn")
            .expect("recovered turn exists");
        assert!(completed.complete);
        let rollout = storage
            .find_activity_rollout("interleaved-a")
            .await
            .expect("load recovered rollout")
            .expect("recovered rollout");
        assert_eq!(rollout.status, "partial");
        assert_eq!(rollout.start_line, completed.start_line as i64);
        assert_eq!(rollout.end_line, completed.end_line as i64);
        assert_eq!(rollout.event_count, completed.event_count as i64);
        assert_eq!(rollout.byte_size, completed.byte_size as i64);
        assert_eq!(rollout.sha256.as_deref(), Some(completed.sha256.as_str()));

        let recovered_activity = storage
            .list_activities(&identity.identity_id, 1)
            .await
            .expect("load recovered activity")
            .pop()
            .expect("recovered activity exists");
        assert_eq!(recovered_activity.status, "failed");
        assert_eq!(
            recovered_activity.error_code.as_deref(),
            Some(ActivityErrorCode::ACTIVITY_INTERRUPTED)
        );
    }

    fn test_activity() -> ActivityInsert {
        ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "responses".to_string(),
            endpoint_name: "Codex".to_string(),
            provider_id: "provider-1".to_string(),
            provider_name: "DeepSeek".to_string(),
            provider_type: "deepseek".to_string(),
            model_requested: "deepseek-flash".to_string(),
            model_upstream: "deepseek-chat".to_string(),
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming: false,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            latency_ms: 1,
            upstream_latency_ms: None,
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
        }
    }
}
