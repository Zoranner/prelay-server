use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::{
    activity::rollout::{RolloutStatus, RolloutSummary},
    entity::{activity_rollouts, identity::activities},
    stats::ActivityInsert,
};

use super::{activities::insert_with_id, Storage, StorageError};

impl Storage {
    pub async fn insert_activity_with_rollout(
        &self,
        identity_id: &str,
        activity_id: String,
        activity: ActivityInsert,
        rollout: RolloutSummary,
    ) -> Result<(), StorageError> {
        let transaction = self.db.begin().await?;
        insert_with_id(&transaction, identity_id, activity_id.clone(), activity).await?;
        insert_rollout(&transaction, identity_id, activity_id, rollout).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn start_stream_activity_with_rollout(
        &self,
        identity_id: &str,
        activity_id: &str,
        activity: ActivityInsert,
        rollout: RolloutSummary,
    ) -> Result<(), StorageError> {
        let transaction = self.db.begin().await?;
        insert_with_id(&transaction, identity_id, activity_id.to_string(), activity).await?;
        insert_rollout(&transaction, identity_id, activity_id.to_string(), rollout).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn complete_activity_rollout(
        &self,
        activity_id: &str,
        rollout: RolloutSummary,
    ) -> Result<(), StorageError> {
        let row = activity_rollouts::Entity::find_by_id(activity_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| StorageError::Rollout("rollout index does not exist".to_string()))?;
        let mut active: activity_rollouts::ActiveModel = row.into();
        active.status = Set(rollout.status.as_str().to_string());
        active.event_count = Set(to_i64(rollout.event_count)?);
        active.byte_size = Set(to_i64(rollout.byte_size)?);
        active.sha256 = Set(Some(rollout.sha256));
        active.completed_at = Set(Some(Utc::now().to_rfc3339()));
        active.update(&self.db).await?;
        Ok(())
    }

    pub async fn find_activity_rollout(
        &self,
        activity_id: &str,
    ) -> Result<Option<activity_rollouts::Model>, StorageError> {
        activity_rollouts::Entity::find_by_id(activity_id)
            .one(&self.db)
            .await
            .map_err(StorageError::from)
    }

    pub async fn list_rollout_paths(&self) -> Result<Vec<String>, StorageError> {
        activity_rollouts::Entity::find()
            .all(&self.db)
            .await
            .map(|rows| rows.into_iter().map(|row| row.rollout_path).collect())
            .map_err(StorageError::from)
    }

    /// Recover rollouts left behind by a terminated process. The rollout index and the
    /// user-visible activity status must move together, otherwise the activity list can
    /// remain permanently stuck in `streaming`.
    pub async fn recover_interrupted_activities(&self) -> Result<u64, StorageError> {
        let transaction = self.db.begin().await?;
        let interrupted = activity_rollouts::Entity::find()
            .filter(activity_rollouts::Column::Status.eq(RolloutStatus::Writing.as_str()))
            .all(&transaction)
            .await?;
        if interrupted.is_empty() {
            transaction.commit().await?;
            return Ok(0);
        }
        let recovered = interrupted.len() as u64;
        let activity_ids = interrupted
            .into_iter()
            .map(|rollout| rollout.activity_id)
            .collect::<Vec<_>>();
        activity_rollouts::Entity::update_many()
            .col_expr(
                activity_rollouts::Column::Status,
                sea_orm::sea_query::Expr::value(RolloutStatus::Partial.as_str()),
            )
            .col_expr(
                activity_rollouts::Column::LastError,
                sea_orm::sea_query::Expr::value("process interrupted before rollout completion"),
            )
            .filter(activity_rollouts::Column::ActivityId.is_in(activity_ids.clone()))
            .exec(&transaction)
            .await?;
        activities::Entity::update_many()
            .col_expr(
                activities::Column::Status,
                sea_orm::sea_query::Expr::value("failed"),
            )
            .col_expr(
                activities::Column::ErrorCode,
                sea_orm::sea_query::Expr::value("activity_interrupted"),
            )
            .col_expr(
                activities::Column::ErrorMessage,
                sea_orm::sea_query::Expr::value(
                    "process interrupted before activity rollout completion",
                ),
            )
            .filter(activities::Column::Id.is_in(activity_ids))
            .filter(activities::Column::Status.eq("streaming"))
            .exec(&transaction)
            .await?;
        transaction.commit().await?;
        Ok(recovered)
    }
}

async fn insert_rollout<C>(
    db: &C,
    identity_id: &str,
    activity_id: String,
    rollout: RolloutSummary,
) -> Result<(), StorageError>
where
    C: sea_orm::ConnectionTrait,
{
    if rollout.activity_id != activity_id {
        return Err(StorageError::Rollout(
            "rollout activity id does not match activity".to_string(),
        ));
    }
    let completed_at = (rollout.status != RolloutStatus::Writing).then(|| Utc::now().to_rfc3339());
    activity_rollouts::ActiveModel {
        activity_id: Set(activity_id),
        identity_id: Set(identity_id.to_string()),
        session_id: Set(rollout.session_id),
        rollout_path: Set(rollout.relative_path),
        status: Set(rollout.status.as_str().to_string()),
        event_count: Set(to_i64(rollout.event_count)?),
        byte_size: Set(to_i64(rollout.byte_size)?),
        sha256: Set(Some(rollout.sha256)),
        created_at: Set(Utc::now().to_rfc3339()),
        completed_at: Set(completed_at),
        last_error: Set(None),
    }
    .insert(db)
    .await?;
    Ok(())
}

pub(super) fn to_i64(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value)
        .map_err(|_| StorageError::Rollout("rollout size exceeds database integer".to_string()))
}

#[cfg(test)]
mod tests {
    use crate::activity::rollout::{RolloutStatus, RolloutSummary};
    use crate::stats::ActivityInsert;
    use crate::storage::RecoveredRollout;

    #[tokio::test]
    async fn inserts_and_completes_rollout_index() {
        let storage = crate::test_support::test_storage().await;
        let identity = storage
            .register_identity(
                "rollout-machine",
                "S-rollout",
                &crate::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity");
        let activity_id = "rollout-activity";
        let rollout = RolloutSummary {
            activity_id: activity_id.to_string(),
            session_id: "session-1".to_string(),
            relative_path: "2026/10/22/rollout-test.jsonl".to_string(),
            status: RolloutStatus::Writing,
            event_count: 2,
            byte_size: 128,
            sha256: "a".repeat(64),
        };
        storage
            .insert_activity_with_rollout(
                &identity.identity_id,
                activity_id.to_string(),
                test_activity(),
                rollout,
            )
            .await
            .expect("insert activity and rollout");

        let found = storage
            .find_activity_rollout(activity_id)
            .await
            .expect("find rollout")
            .expect("rollout exists");
        assert_eq!(found.status, "writing");
        assert_eq!(found.session_id, "session-1");

        storage
            .complete_activity_rollout(
                activity_id,
                RolloutSummary {
                    activity_id: activity_id.to_string(),
                    session_id: "session-1".to_string(),
                    relative_path: "2026/10/22/rollout-test.jsonl".to_string(),
                    status: RolloutStatus::Completed,
                    event_count: 4,
                    byte_size: 256,
                    sha256: "b".repeat(64),
                },
            )
            .await
            .expect("complete rollout");
        let found = storage
            .find_activity_rollout(activity_id)
            .await
            .expect("find completed rollout")
            .expect("completed rollout exists");
        assert_eq!(found.status, "completed");
        assert_eq!(found.event_count, 4);
        assert!(found.completed_at.is_some());
    }

    #[tokio::test]
    async fn inserts_a_final_rollout_with_its_completed_status() {
        let storage = crate::test_support::test_storage().await;
        let identity = storage
            .register_identity(
                "rollout-final-machine",
                "S-rollout-final",
                &crate::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity");
        let activity_id = "completed-rollout-activity";
        storage
            .insert_activity_with_rollout(
                &identity.identity_id,
                activity_id.to_string(),
                test_activity(),
                RolloutSummary {
                    activity_id: activity_id.to_string(),
                    session_id: "session-final".to_string(),
                    relative_path: "2026/10/22/rollout-final.jsonl".to_string(),
                    status: RolloutStatus::Completed,
                    event_count: 3,
                    byte_size: 192,
                    sha256: "c".repeat(64),
                },
            )
            .await
            .expect("insert completed rollout");

        let found = storage
            .find_activity_rollout(activity_id)
            .await
            .expect("find completed rollout")
            .expect("completed rollout exists");
        assert_eq!(found.status, "completed");
        assert!(found.completed_at.is_some());
    }

    #[tokio::test]
    async fn recovers_interrupted_activity_and_rollout_status_together() {
        let storage = crate::test_support::test_storage().await;
        let identity = storage
            .register_identity(
                "recover-machine",
                "S-recover",
                &crate::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity");
        let activity_id = "interrupted-activity";
        let mut activity = test_activity();
        activity.status = "streaming".to_string();
        activity.is_streaming = true;
        storage
            .insert_activity_with_rollout(
                &identity.identity_id,
                activity_id.to_string(),
                activity,
                RolloutSummary {
                    activity_id: activity_id.to_string(),
                    session_id: "session-interrupted".to_string(),
                    relative_path: "2026/10/22/rollout-interrupted.jsonl".to_string(),
                    status: RolloutStatus::Writing,
                    event_count: 2,
                    byte_size: 128,
                    sha256: "d".repeat(64),
                },
            )
            .await
            .expect("insert interrupted activity");

        assert_eq!(
            storage
                .recover_interrupted_activities()
                .await
                .expect("recover interrupted activity"),
            1
        );

        let activity = storage
            .list_activities(&identity.identity_id, 1)
            .await
            .expect("load recovered activity")
            .pop()
            .expect("recovered activity");
        assert_eq!(activity.status, "failed");
        assert_eq!(activity.error_code.as_deref(), Some("activity_interrupted"));
        let rollout = storage
            .find_activity_rollout(activity_id)
            .await
            .expect("load recovered rollout")
            .expect("recovered rollout");
        assert_eq!(rollout.status, "partial");
        assert!(rollout.last_error.is_some());
    }

    #[tokio::test]
    async fn applies_a_verified_recovered_rollout_to_both_tables() {
        let storage = crate::test_support::test_storage().await;
        let identity = storage
            .register_identity(
                "recover-completed-machine",
                "S-recover-completed",
                &crate::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity");
        let activity_id = "recover-completed-activity";
        let mut activity = test_activity();
        activity.status = "streaming".to_string();
        activity.is_streaming = true;
        storage
            .insert_activity_with_rollout(
                &identity.identity_id,
                activity_id.to_string(),
                activity,
                RolloutSummary {
                    activity_id: activity_id.to_string(),
                    session_id: "session-recovered".to_string(),
                    relative_path: "2026/10/23/rollout-recovered.jsonl".to_string(),
                    status: RolloutStatus::Writing,
                    event_count: 0,
                    byte_size: 0,
                    sha256: String::new(),
                },
            )
            .await
            .expect("insert writing activity");

        storage
            .recover_rollout_status(RecoveredRollout {
                activity_id: activity_id.to_string(),
                status: RolloutStatus::Completed,
                http_status: Some(200),
                error_code: None,
                error_message: None,
                event_count: Some(4),
                byte_size: Some(512),
                sha256: Some("e".repeat(64)),
            })
            .await
            .expect("recover completed rollout");

        let activity = storage
            .list_activities(&identity.identity_id, 1)
            .await
            .expect("load recovered activity")
            .pop()
            .expect("recovered activity");
        assert_eq!(activity.status, "success");
        assert_eq!(activity.http_status, Some(200));
        let rollout = storage
            .find_activity_rollout(activity_id)
            .await
            .expect("load recovered rollout")
            .expect("recovered rollout");
        assert_eq!(rollout.status, "completed");
        assert_eq!(rollout.event_count, 4);
        assert_eq!(rollout.byte_size, 512);
        assert!(rollout.completed_at.is_some());
    }

    fn test_activity() -> ActivityInsert {
        ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "responses".to_string(),
            endpoint_name: "Codex".to_string(),
            provider_id: "provider-1".to_string(),
            provider_name: "DeepSeek".to_string(),
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
