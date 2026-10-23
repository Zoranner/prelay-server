use axum::Router;
use prelay_server::{
    app, client_update::ClientUpdateCache, extensions::ExtensionCatalog,
    provider_catalog::ProviderCatalog, AppState,
};

use crate::support::TestStorage;

pub struct TestContext {
    pub app: Router,
    pub storage: TestStorage,
    #[allow(dead_code)]
    pub activity_store: prelay_server::activity::ActivityStore,
}

pub async fn test_context() -> TestContext {
    test_context_with_catalog(prelay_server::test_support::fixture_catalog()).await
}

pub async fn test_context_with_catalog(provider_catalog: ProviderCatalog) -> TestContext {
    let storage = crate::support::test_storage().await;
    let rollout_store = prelay_server::activity::rollout::RolloutStore::new(
        std::env::temp_dir().join(format!("prelay-rollout-test-{}", uuid::Uuid::new_v4())),
    );
    let activity_store =
        prelay_server::activity::ActivityStore::new(storage.storage().clone(), rollout_store);
    let app = app::router(AppState {
        provider_catalog: std::sync::Arc::new(provider_catalog),
        storage: storage.storage().clone(),
        activity_store: activity_store.clone(),
        client: reqwest::Client::new(),
        provider_clients: Default::default(),
        client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
        extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
        failover: Default::default(),
    })
    .await
    .expect("build application router");
    TestContext {
        app,
        storage,
        activity_store,
    }
}
