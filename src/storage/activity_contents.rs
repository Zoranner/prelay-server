use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use sea_query::{Expr, Query};
use std::time::Duration;
use uuid::Uuid;

use crate::{
    activity::ActivityContentDraft,
    entity::{activity_contents, identity::activities},
};

use super::{Storage, StorageError};

impl Storage {
    pub async fn enqueue_activity_content(
        &self,
        draft: ActivityContentDraft,
    ) -> Result<(), StorageError> {
        insert(&self.db, draft, "pending").await
    }

    pub async fn find_activity_content(
        &self,
        activity_id: &str,
    ) -> Result<Option<activity_contents::Model>, StorageError> {
        activity_contents::Entity::find()
            .filter(activity_contents::Column::ActivityId.eq(activity_id))
            .one(&self.db)
            .await
            .map_err(StorageError::from)
    }

    pub async fn recover_stale_activity_content(
        &self,
        max_age: Duration,
    ) -> Result<u64, StorageError> {
        let now = Utc::now();
        let cutoff = now
            - chrono::Duration::from_std(max_age)
                .map_err(|error| StorageError::ValidationFailed(error.to_string()))?;
        let result = activity_contents::Entity::update_many()
            .col_expr(activity_contents::Column::Status, Expr::value("pending"))
            .col_expr(
                activity_contents::Column::NextAttemptAt,
                Expr::value(now.to_rfc3339()),
            )
            .col_expr(
                activity_contents::Column::LeaseOwner,
                Expr::value(Option::<String>::None),
            )
            .col_expr(
                activity_contents::Column::LeaseExpiresAt,
                Expr::value(Option::<String>::None),
            )
            .col_expr(
                activity_contents::Column::CompletedAt,
                Expr::value(now.to_rfc3339()),
            )
            .filter(activity_contents::Column::Status.eq("capturing"))
            .filter(activity_contents::Column::UpdatedAt.lt(cutoff.to_rfc3339()))
            .filter(
                activity_contents::Column::ActivityId.in_subquery(
                    Query::select()
                        .column(activities::Column::Id)
                        .from(activities::Entity)
                        .and_where(activities::Column::Status.is_in(["success", "failed"]))
                        .take(),
                ),
            )
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }
}

pub(super) async fn insert<C>(
    db: &C,
    draft: ActivityContentDraft,
    status: &str,
) -> Result<(), StorageError>
where
    C: ConnectionTrait,
{
    let now = Utc::now().to_rfc3339();
    activity_contents::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        activity_id: Set(draft.activity_id),
        input_text: Set(draft.input_text),
        output_text: Set(draft.output_text),
        media_metadata_json: Set(draft.media_metadata_json),
        is_truncated: Set(draft.is_truncated),
        content_hash: Set(draft.content_hash),
        status: Set(status.to_string()),
        attempts: Set(0),
        next_attempt_at: Set(Some(now.clone())),
        lease_owner: Set(None),
        lease_expires_at: Set(None),
        last_error: Set(None),
        completed_at: Set(None),
        created_at: Set(now.clone()),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    Ok(())
}

pub(super) async fn complete<C>(db: &C, draft: ActivityContentDraft) -> Result<(), StorageError>
where
    C: ConnectionTrait,
{
    let Some(row) = activity_contents::Entity::find()
        .filter(activity_contents::Column::ActivityId.eq(&draft.activity_id))
        .one(db)
        .await?
    else {
        return insert(db, draft, "pending").await;
    };
    let now = Utc::now().to_rfc3339();
    let mut active: activity_contents::ActiveModel = row.into();
    active.input_text = Set(draft.input_text);
    active.output_text = Set(draft.output_text);
    active.media_metadata_json = Set(draft.media_metadata_json);
    active.is_truncated = Set(draft.is_truncated);
    active.content_hash = Set(draft.content_hash);
    active.status = Set("pending".to_string());
    active.completed_at = Set(Some(now.clone()));
    active.updated_at = Set(now);
    active.update(db).await?;
    Ok(())
}
