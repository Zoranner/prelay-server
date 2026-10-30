use crate::{
    domain::activity::RolloutStatus, domain::stats::StreamActivityUpdate,
    service::activity::ActivityStore, store::storage::StorageError,
};

pub(super) async fn update_stream_log(
    activity_store: &ActivityStore,
    identity_id: &str,
    id: &str,
    update: StreamActivityUpdate,
) {
    if let Err(error) = activity_store
        .update_stream_activity(identity_id, id, update)
        .await
    {
        log_stream_storage_failure("update", &error);
    }
}

pub(super) fn log_stream_storage_failure(operation: &'static str, _error: &StorageError) {
    tracing::error!(
        operation,
        failure_kind = "stream_log_storage",
        "failed to persist streaming activity"
    );
}

pub(super) fn final_rollout_status(status: RolloutStatus, append_failed: bool) -> RolloutStatus {
    if append_failed && status == RolloutStatus::Completed {
        RolloutStatus::Partial
    } else {
        status
    }
}

#[cfg(test)]
mod tests {
    use super::final_rollout_status;
    use crate::domain::activity::RolloutStatus;

    #[test]
    fn append_failure_downgrades_completed_to_partial() {
        assert_eq!(
            final_rollout_status(RolloutStatus::Completed, true),
            RolloutStatus::Partial
        );
        assert_eq!(
            final_rollout_status(RolloutStatus::Failed, true),
            RolloutStatus::Failed
        );
    }
}
