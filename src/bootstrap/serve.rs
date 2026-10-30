use std::{path::Path, sync::Arc};

use crate::{
    domain::catalog::ProviderCatalog,
    inbound,
    outbound::{
        client,
        integrations::{
            client_update::{self, ClientUpdateCache},
            extensions::ExtensionCatalog,
        },
    },
    service::{activity::ActivityStore, cleanup::delete_expired_identities},
    store::{
        connect::{connect, DatabaseConfig},
        rollout::RolloutStore,
        schema::initialize_with_catalog,
        storage::{MasterKey, Storage},
    },
    AppState,
};

use super::{
    diagnostics::{
        handle_startup_cleanup_failure, warn_incomplete_rollout_files, warn_orphan_rollout_files,
    },
    environment::{catalog_directory, listen_address, load_environment_file},
    router::router,
    tasks::{spawn_client_update_refresh, spawn_identity_cleanup},
};

pub async fn serve() -> anyhow::Result<()> {
    load_environment_file(Path::new(".env"))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "prelay_server=info,tower_http=info".into()),
        )
        .init();

    inbound::limits::initialize_from_environment().map_err(anyhow::Error::msg)?;
    let provider_catalog = Arc::new(ProviderCatalog::load(&catalog_directory())?);
    let database_config = DatabaseConfig::from_environment()?;
    let db = connect(&database_config).await?;
    initialize_with_catalog(&db, &provider_catalog).await?;
    let storage = Storage::from_connection(db, MasterKey::from_environment()?);
    let rollout_store = RolloutStore::from_environment().map_err(anyhow::Error::msg)?;
    let activity_store = ActivityStore::new(storage.clone(), rollout_store);
    activity_store
        .validate()
        .map_err(|error| anyhow::anyhow!("activity rollout directory is not writable: {error}"))?;
    match activity_store.recover_interrupted().await {
        Ok(recovered) if recovered > 0 => {
            tracing::warn!(
                recovered,
                "marked interrupted activity rollouts as partial at startup"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::error!(
                error_code = error.code().as_str(),
                "failed to mark interrupted activity rollouts as partial"
            );
        }
    }
    warn_incomplete_rollout_files(activity_store.rollouts()).await;
    warn_orphan_rollout_files(&activity_store).await;
    let deleted = delete_expired_identities(&activity_store)
        .await
        .map_err(handle_startup_cleanup_failure)?;
    if deleted > 0 {
        tracing::info!(deleted, "deleted inactive identities at startup");
    }
    spawn_identity_cleanup(activity_store.clone());

    let upstream_policy = client::initialize_from_environment().map_err(anyhow::Error::msg)?;
    // 图片预算的默认值按协议入口上限推导，入口上限已在启动时先初始化。
    inbound::v1::initialize_image_budget_from_environment().map_err(anyhow::Error::msg)?;
    let client = client::build_client(upstream_policy)?;
    let client_update_client = client_update::http_client_from_environment(
        upstream_policy.connect_timeout,
        upstream_policy.read_timeout,
    )?;
    let client_update = ClientUpdateCache::from_environment(client_update_client).await?;
    spawn_client_update_refresh(client_update.clone());
    let extensions = ExtensionCatalog::from_environment(client.clone())?;
    extensions.warm();

    let state = AppState {
        provider_catalog,
        storage,
        activity_store,
        client,
        provider_clients: Default::default(),
        client_update,
        extensions,
        failover: Default::default(),
    };
    let app = router(state).await?;

    let configured_listen_address = std::env::var("LISTEN_ADDRESS").ok();
    let addr = listen_address(configured_listen_address.as_deref())?;

    tracing::info!(%addr, "Prelay server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
