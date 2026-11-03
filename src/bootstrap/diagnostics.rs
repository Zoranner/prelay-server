use crate::{
    service::activity::ActivityStore, store::rollout::RolloutStore, store::storage::StorageError,
};

const STARTUP_CLEANUP_FAILURE: &str = "startup identity cleanup failed";
const STARTUP_RESPONSE_STATE_CLEANUP_FAILURE: &str = "startup response state cleanup failed";
const STARTUP_LEGACY_SESSION_ENCRYPTION_FAILURE: &str =
    "startup legacy response session encryption failed";

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

pub(super) fn log_response_state_cleanup_failure(error: &StorageError) {
    tracing::warn!(
        error_code = error.code().as_str(),
        "failed to delete expired response states"
    );
}

pub(super) fn handle_startup_response_state_cleanup_failure(error: StorageError) -> anyhow::Error {
    tracing::error!(
        error_code = error.code().as_str(),
        failure_kind = "startup_response_state_cleanup",
        "failed to delete expired response states at startup"
    );
    anyhow::anyhow!(STARTUP_RESPONSE_STATE_CLEANUP_FAILURE)
}

pub(super) fn handle_startup_legacy_session_encryption_failure(
    error: StorageError,
) -> anyhow::Error {
    tracing::error!(
        error_code = error.code().as_str(),
        failure_kind = "startup_legacy_session_encryption",
        "failed to encrypt legacy response session content at startup"
    );
    anyhow::anyhow!(STARTUP_LEGACY_SESSION_ENCRYPTION_FAILURE)
}

pub(super) async fn warn_incomplete_rollout_files(rollout_store: &RolloutStore) {
    let store = rollout_store.clone();
    match tokio::task::spawn_blocking(move || store.incomplete_rollout_files()).await {
        Ok(Ok(incomplete_tails)) if incomplete_tails.is_empty() => {}
        Ok(Ok(incomplete_tails)) => {
            tracing::warn!(
                incomplete = incomplete_tails.len(),
                sample = ?incomplete_tails.iter().take(10).collect::<Vec<_>>(),
                "activity rollout files have incomplete tails; the next request for the thread will truncate them"
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
