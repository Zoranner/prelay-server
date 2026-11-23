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
    service::{
        activity::ActivityStore,
        cleanup::delete_expired_identities,
        recording::{RecordingRuntime, RecordingRuntimeConfig},
    },
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
        handle_startup_cleanup_failure, handle_startup_response_state_cleanup_failure,
        warn_incomplete_rollout_files, warn_orphan_rollout_files,
    },
    environment::{catalog_directory, listen_address, load_environment_file},
    router::router,
    tasks::{spawn_client_update_refresh, spawn_identity_cleanup, spawn_response_state_cleanup},
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
    let master_key = MasterKey::from_environment()?;
    let storage = Storage::from_connection(db, master_key.clone());
    let rollout_store = RolloutStore::from_environment().map_err(anyhow::Error::msg)?;
    rollout_store
        .validate()
        .map_err(|error| anyhow::anyhow!("activity rollout directory is not writable: {error}"))?;
    let recording = RecordingRuntime::start(
        RecordingRuntimeConfig::from_environment()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
        &master_key,
        storage.clone(),
        rollout_store.clone(),
    )
    .await
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let activity_store = ActivityStore::new(storage.clone(), rollout_store);
    activity_store
        .validate()
        .map_err(|error| anyhow::anyhow!("activity rollout directory is not writable: {error}"))?;
    match activity_store.recover_interrupted().await {
        Ok(recovered) if recovered > 0 => {
            tracing::warn!(
                recovered,
                "reconciled interrupted activity rollouts at startup"
            );
        }
        Ok(_) => {}
        Err(error) => {
            return Err(anyhow::anyhow!(
                "failed to recover interrupted activity rollouts: {} ({})",
                error.code().as_str(),
                error
            ));
        }
    }
    warn_incomplete_rollout_files(activity_store.rollouts()).await;
    warn_orphan_rollout_files(&activity_store).await;
    recording.start_materialization();
    let deleted = delete_expired_identities(&activity_store)
        .await
        .map_err(handle_startup_cleanup_failure)?;
    if deleted > 0 {
        tracing::info!(deleted, "deleted inactive identities at startup");
    }
    spawn_identity_cleanup(activity_store.clone());
    let deleted = storage
        .delete_expired_response_states(chrono::Utc::now())
        .await
        .map_err(handle_startup_response_state_cleanup_failure)?;
    if deleted > 0 {
        tracing::info!(deleted, "deleted expired response states at startup");
    }
    spawn_response_state_cleanup(storage.clone());

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
    let (durable_sequence, applied_sequence) = recording.sequence_handles();
    let mut quota =
        crate::service::quota::QuotaService::new(client.clone(), provider_catalog.clone());
    quota.track_recording_sequences(durable_sequence, applied_sequence);

    let recording_for_shutdown = recording.clone();
    let state = AppState {
        provider_catalog,
        storage,
        activity_store,
        client,
        provider_clients: Default::default(),
        client_update,
        extensions,
        failover: Default::default(),
        quota,
        recording: Some(recording),
    };
    let app = router(state).await?;

    let configured_listen_address = std::env::var("LISTEN_ADDRESS").ok();
    let addr = listen_address(configured_listen_address.as_deref())?;

    tracing::info!(%addr, "Prelay server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    recording_for_shutdown.shutdown().await;
    serve_result?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
