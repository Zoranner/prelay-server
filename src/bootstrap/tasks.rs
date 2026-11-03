use crate::{
    outbound::integrations::client_update::ClientUpdateCache, service::activity::ActivityStore,
    service::cleanup::delete_expired_identities, store::storage::Storage,
};

use super::diagnostics::{log_cleanup_failure, log_response_state_cleanup_failure};

pub(super) fn spawn_identity_cleanup(activity_store: ActivityStore) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            match delete_expired_identities(&activity_store).await {
                Ok(deleted) if deleted > 0 => {
                    tracing::info!(deleted, "deleted inactive identities");
                }
                Ok(_) => {}
                Err(error) => log_cleanup_failure(&error),
            }
        }
    });
}

pub(super) fn spawn_response_state_cleanup(storage: Storage) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            match storage
                .delete_expired_response_states(chrono::Utc::now())
                .await
            {
                Ok(deleted) if deleted > 0 => {
                    tracing::info!(deleted, "deleted expired response states");
                }
                Ok(_) => {}
                Err(error) => log_response_state_cleanup_failure(&error),
            }
        }
    });
}

pub(super) fn spawn_client_update_refresh(client_update: ClientUpdateCache) {
    // 首次刷新在后台立即开始：服务先进入监听，不等待下载完成。
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(6 * 60 * 60));
        loop {
            interval.tick().await;
            if let Err(error) = client_update.refresh().await {
                tracing::warn!(error = %error, "failed to refresh client update cache");
            }
        }
    });
}
