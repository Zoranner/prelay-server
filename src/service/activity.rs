use chrono::{Duration, Utc};
use std::collections::HashSet;
use std::time::Instant;
use uuid::Uuid;

use crate::{
    domain::activity::{
        rollout::{RolloutStatus, SessionContext, TurnContext},
        ContentDraft, ContentEventBody, ConversationError,
    },
    domain::stats::ActivityInsert,
    store::{
        recording_timing::log_elapsed,
        rollout::{RolloutStore, RolloutWriter},
        storage::{ResponseStateInsert, Storage, StorageError},
    },
};

mod recovery;

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
        session: &SessionContext,
        request_events: Vec<ContentDraft>,
        response_events: Vec<ContentDraft>,
    ) -> Result<(), StorageError> {
        let started_at = Instant::now();
        let activity_id = Uuid::new_v4().to_string();
        let begin_started_at = Instant::now();
        let mut writer = self
            .begin_rollout(
                &activity_id,
                identity_id,
                &activity,
                session,
                request_events,
            )
            .await?;
        log_elapsed(
            "activity.record_completed.begin_rollout",
            begin_started_at,
            Some(&activity_id),
        );
        for event in response_events {
            writer
                .append_content(event)
                .map_err(|error| StorageError::Rollout(error.to_string()))?;
        }
        if let Some(message) = activity.error_message.as_deref() {
            writer
                .append_content(ContentDraft::new(ContentEventBody::Error(
                    ConversationError {
                        code: activity.error_code.clone(),
                        message: message.to_string(),
                        http_status: u16::try_from(activity.http_status).ok(),
                    },
                )))
                .map_err(|error| StorageError::Rollout(error.to_string()))?;
        }
        let status = if activity.status == "failed" {
            RolloutStatus::Failed
        } else {
            RolloutStatus::Completed
        };
        let finish_started_at = Instant::now();
        let summary = writer
            .finish_with(
                status,
                u16::try_from(activity.http_status).ok(),
                activity.error_code.clone(),
                activity.error_message.clone(),
            )
            .map_err(|error| StorageError::Rollout(error.to_string()))?;
        log_elapsed(
            "activity.record_completed.finish_rollout",
            finish_started_at,
            Some(&activity_id),
        );
        let db_started_at = Instant::now();
        let result = self
            .storage
            .insert_activity_with_rollout(identity_id, activity_id.clone(), activity, summary)
            .await;
        if result.is_ok() {
            log_elapsed(
                "activity.record_completed.db_insert",
                db_started_at,
                Some(&activity_id),
            );
            log_elapsed(
                "activity.record_completed.total",
                started_at,
                Some(&activity_id),
            );
        }
        result
    }

    pub async fn begin_stream(
        &self,
        identity_id: &str,
        activity: &ActivityInsert,
        session: &SessionContext,
        request_events: Vec<ContentDraft>,
    ) -> Result<(String, RolloutWriter), StorageError> {
        let started_at = Instant::now();
        let activity_id = Uuid::new_v4().to_string();
        let writer = self
            .begin_rollout(&activity_id, identity_id, activity, session, request_events)
            .await?;
        log_elapsed("activity.begin_stream", started_at, Some(&activity_id));
        Ok((activity_id, writer))
    }

    pub async fn start_stream(
        &self,
        identity_id: &str,
        activity_id: &str,
        activity: ActivityInsert,
        summary: crate::domain::activity::rollout::RolloutSummary,
    ) -> Result<(), StorageError> {
        let started_at = Instant::now();
        let result = self
            .storage
            .start_stream_activity_with_rollout(identity_id, activity_id, activity, summary)
            .await;
        if result.is_ok() {
            log_elapsed(
                "activity.start_stream.db_insert",
                started_at,
                Some(activity_id),
            );
        }
        result
    }

    pub async fn insert_stream_result(
        &self,
        identity_id: &str,
        activity_id: String,
        activity: ActivityInsert,
        summary: crate::domain::activity::rollout::RolloutSummary,
    ) -> Result<(), StorageError> {
        let started_at = Instant::now();
        let result = self
            .storage
            .insert_activity_with_rollout(identity_id, activity_id, activity, summary)
            .await;
        if result.is_ok() {
            log_elapsed("activity.insert_stream_result.db_insert", started_at, None);
        }
        result
    }

    pub async fn complete_stream(
        &self,
        activity_id: &str,
        summary: crate::domain::activity::rollout::RolloutSummary,
    ) -> Result<(), StorageError> {
        let started_at = Instant::now();
        let result = self
            .storage
            .complete_activity_rollout(activity_id, summary)
            .await;
        if result.is_ok() {
            log_elapsed(
                "activity.complete_stream.db_update",
                started_at,
                Some(activity_id),
            );
        }
        result
    }

    pub async fn update_stream_activity(
        &self,
        identity_id: &str,
        activity_id: &str,
        update: crate::domain::stats::StreamActivityUpdate,
    ) -> Result<(), StorageError> {
        let started_at = Instant::now();
        let result = self
            .storage
            .update_stream_activity(identity_id, activity_id, update)
            .await;
        if result.is_ok() {
            log_elapsed(
                "activity.update_stream_activity.db_update",
                started_at,
                Some(activity_id),
            );
        }
        result
    }

    pub async fn save_response_state(
        &self,
        insert: ResponseStateInsert<'_>,
    ) -> Result<(), StorageError> {
        let response_id = insert.response_id.to_string();
        let started_at = Instant::now();
        let result = self.storage.save_response_state(insert).await;
        if result.is_ok() {
            log_elapsed(
                "activity.save_response_state.db_insert",
                started_at,
                Some(&response_id),
            );
        }
        result
    }

    async fn begin_rollout(
        &self,
        activity_id: &str,
        identity_id: &str,
        activity: &ActivityInsert,
        session: &SessionContext,
        request_events: Vec<ContentDraft>,
    ) -> Result<RolloutWriter, StorageError> {
        let turn = TurnContext {
            activity_id: activity_id.to_string(),
            model_requested: activity.model_requested.clone(),
            model_upstream: activity.model_upstream.clone(),
        };
        self.rollouts
            .begin(identity_id, session, turn, request_events)
            .await
            .map_err(|error| StorageError::Rollout(error.to_string()))
    }

    async fn remove_rollout_files(&self, relative_paths: &[String]) {
        let mut seen = HashSet::new();
        for relative_path in relative_paths {
            if !seen.insert(relative_path.as_str()) {
                continue;
            }
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
            self.rollouts.remove_thread_index(relative_path);
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {
                    if let Some(parent) = path.parent() {
                        if parent != self.rollouts.root() {
                            let _ = tokio::fs::remove_dir(parent).await;
                        }
                    }
                }
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
        self.rollouts.forget_relative_paths(relative_paths);
    }
}
