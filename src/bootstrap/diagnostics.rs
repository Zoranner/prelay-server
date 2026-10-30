use crate::{
    service::activity::ActivityStore, store::rollout::RolloutStore, store::storage::StorageError,
};

const STARTUP_CLEANUP_FAILURE: &str = "startup identity cleanup failed";

pub(super) fn log_cleanup_failure(error: &StorageError) {
    tracing::warn!(
        error_code = error.code().as_str(),
        "failed to delete inactive identities"
    );
}

pub(super) fn handle_startup_cleanup_failure(error: StorageError) -> anyhow::Error {
    tracing::error!(
        error_code = error.code().as_str(),
        failure_kind = "startup_identity_cleanup",
        "failed to delete inactive identities at startup"
    );
    anyhow::anyhow!(STARTUP_CLEANUP_FAILURE)
}

pub(super) async fn warn_incomplete_rollout_files(rollout_store: &RolloutStore) {
    let store = rollout_store.clone();
    match tokio::task::spawn_blocking(move || store.part_files()).await {
        Ok(Ok(paths)) if paths.is_empty() => {}
        Ok(Ok(paths)) => {
            tracing::warn!(
                incomplete = paths.len(),
                sample = ?paths.iter().take(10).collect::<Vec<_>>(),
                "incomplete activity rollout files remain on disk"
            );
        }
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error,
                "failed to scan incomplete activity rollout files"
            );
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                "incomplete activity rollout scan task failed"
            );
        }
    }
}

pub(super) async fn warn_orphan_rollout_files(activity_store: &ActivityStore) {
    match activity_store.orphan_rollout_files().await {
        Ok(paths) if paths.is_empty() => {}
        Ok(paths) => {
            tracing::warn!(
                orphans = paths.len(),
                sample = ?paths.iter().take(10).collect::<Vec<_>>(),
                "completed activity rollout files without an index were found"
            );
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                "failed to scan orphan activity rollout files"
            );
        }
    }
}
