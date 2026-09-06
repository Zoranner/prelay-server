use uuid::Uuid;

use crate::{
    activity::{ActivityContentDraft, NormalizedActivityContent},
    stats::ActivityInsert,
};
use sea_orm::TransactionTrait;

use super::{activities, activity_contents, Storage, StorageError};

impl Storage {
    pub async fn record_completed_activity(
        &self,
        identity_id: &str,
        activity: ActivityInsert,
        content: NormalizedActivityContent,
    ) -> Result<String, StorageError> {
        let activity_id = Uuid::new_v4().to_string();
        let transaction = self.db.begin().await?;
        activities::insert_with_id(&transaction, identity_id, activity_id.clone(), activity)
            .await?;
        activity_contents::insert(
            &transaction,
            content.into_draft(activity_id.clone()),
            "pending",
        )
        .await?;
        transaction.commit().await?;
        Ok(activity_id)
    }

    pub async fn start_stream_activity(
        &self,
        identity_id: &str,
        activity_id: &str,
        activity: ActivityInsert,
        content: NormalizedActivityContent,
    ) -> Result<(), StorageError> {
        let transaction = self.db.begin().await?;
        activities::insert_with_id(&transaction, identity_id, activity_id.to_string(), activity)
            .await?;
        activity_contents::insert(
            &transaction,
            content.into_draft(activity_id.to_string()),
            "capturing",
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn complete_stream_activity_content(
        &self,
        content: ActivityContentDraft,
    ) -> Result<(), StorageError> {
        activity_contents::complete(&self.db, content).await
    }
}
