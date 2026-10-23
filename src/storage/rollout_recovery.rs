use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::{
    activity::rollout::RolloutStatus,
    entity::{activity_rollouts, identity::activities},
};

use super::{rollouts::to_i64, Storage, StorageError};

pub struct InterruptedRollout {
    pub activity_id: String,
    pub rollout_path: String,
}

pub struct RecoveredRollout {
    pub activity_id: String,
    pub status: RolloutStatus,
    pub http_status: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub event_count: Option<u64>,
    pub byte_size: Option<u64>,
    pub sha256: Option<String>,
}

impl Storage {
    pub async fn list_writing_rollouts(&self) -> Result<Vec<InterruptedRollout>, StorageError> {
        activity_rollouts::Entity::find()
            .filter(activity_rollouts::Column::Status.eq(RolloutStatus::Writing.as_str()))
            .all(&self.db)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|row| InterruptedRollout {
                        activity_id: row.activity_id,
                        rollout_path: row.rollout_path,
                    })
                    .collect()
            })
            .map_err(StorageError::from)
    }

    pub async fn recover_rollout_status(
        &self,
        update: RecoveredRollout,
    ) -> Result<(), StorageError> {
        let transaction = self.db.begin().await?;
        let rollout = activity_rollouts::Entity::find_by_id(&update.activity_id)
            .one(&transaction)
            .await?
            .ok_or_else(|| StorageError::Rollout("rollout index does not exist".to_string()))?;
        let mut active: activity_rollouts::ActiveModel = rollout.into();
        active.status = Set(update.status.as_str().to_string());
        active.completed_at = Set(Some(Utc::now().to_rfc3339()));
        active.last_error = Set(if update.status == RolloutStatus::Completed {
            None
        } else {
            update.error_message.clone()
        });
        if let Some(event_count) = update.event_count {
            active.event_count = Set(to_i64(event_count)?);
        }
        if let Some(byte_size) = update.byte_size {
            active.byte_size = Set(to_i64(byte_size)?);
        }
        if let Some(sha256) = update.sha256 {
            active.sha256 = Set(Some(sha256));
        }
        active.update(&transaction).await?;

        let activity = activities::Entity::find_by_id(&update.activity_id)
            .one(&transaction)
            .await?
            .ok_or_else(|| StorageError::Rollout("activity does not exist".to_string()))?;
        let mut active: activities::ActiveModel = activity.into();
        active.status = Set(if update.status == RolloutStatus::Completed {
            "success".to_string()
        } else {
            "failed".to_string()
        });
        active.is_streaming = Set(Some(false));
        active.http_status = Set(update.http_status);
        active.error_code = Set(update.error_code);
        active.error_message = Set(update.error_message);
        active.update(&transaction).await?;
        transaction.commit().await?;
        Ok(())
    }
}
