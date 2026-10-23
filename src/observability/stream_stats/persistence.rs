use crate::{activity::ActivityStore, stats::StreamActivityUpdate, storage::StorageError};

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
