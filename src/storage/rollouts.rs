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
        active.thread_id = Set(rollout.thread_id);
        active.window_id = Set(rollout.window_id);
        active.generation = Set(to_i64(rollout.generation)?);
        active.start_line = Set(to_i64(rollout.start_line)?);
        active.end_line = Set(to_i64(rollout.end_line)?);
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
        session_id: Set(rollout.thread_id.clone()),
        thread_id: Set(rollout.thread_id),
        window_id: Set(rollout.window_id),
        generation: Set(to_i64(rollout.generation)?),
        rollout_path: Set(rollout.relative_path),
        status: Set(rollout.status.as_str().to_string()),
        start_line: Set(to_i64(rollout.start_line)?),
        end_line: Set(to_i64(rollout.end_line)?),
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
#[path = "rollouts_tests.rs"]
mod tests;
