use crate::{
    stats::{ActivityInsert, StreamActivityUpdate},
    storage::{Storage, StorageError},
};

pub(super) async fn insert_stream_log_with_id(
    storage: &Storage,
    identity_id: &str,
    id: &str,
    log: ActivityInsert,
) -> Result<(), StorageError> {
    if let Err(error) = storage
        .insert_activity_with_id(identity_id, id.to_string(), log)
        .await
    {
        log_stream_storage_failure("insert", &error);
        return Err(error);
    }
    Ok(())
}

pub(super) async fn update_stream_log(
    storage: &Storage,
    identity_id: &str,
    id: &str,
    update: StreamActivityUpdate,
) {
    if let Err(error) = storage
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
