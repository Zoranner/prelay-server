use axum::Router;
use prelay_server::{
    bootstrap as app, domain::catalog::ProviderCatalog,
    outbound::integrations::client_update::ClientUpdateCache,
    outbound::integrations::extensions::ExtensionCatalog, AppState,
};

use crate::support::TestStorage;

pub struct TestContext {
    pub app: Router,
    pub storage: TestStorage,
    #[allow(dead_code)]
    pub activity_store: prelay_server::service::activity::ActivityStore,
}

pub async fn test_context() -> TestContext {
    test_context_with_catalog(prelay_server::test_support::fixture_catalog()).await
}

pub async fn test_context_with_catalog(provider_catalog: ProviderCatalog) -> TestContext {
    let storage = crate::support::test_storage().await;
    let rollout_store = prelay_server::store::rollout::RolloutStore::new(
        std::env::temp_dir().join(format!("prelay-rollout-test-{}", uuid::Uuid::new_v4())),
    );
    let activity_store = prelay_server::service::activity::ActivityStore::new(
        storage.storage().clone(),
        rollout_store,
    );
    let provider_catalog = std::sync::Arc::new(provider_catalog);
    let app = app::router(AppState {
        provider_catalog: provider_catalog.clone(),
        storage: storage.storage().clone(),
        activity_store: activity_store.clone(),
        client: reqwest::Client::new(),
        provider_clients: Default::default(),
        client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
        extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
        failover: Default::default(),
        session_routes: Default::default(),
        quota: prelay_server::service::quota::QuotaService::new(
            reqwest::Client::new(),
            provider_catalog,
        ),
        recording: None,
    })
    .await
    .expect("build application router");
    TestContext {
        app,
        storage,
        activity_store,
    }
}
