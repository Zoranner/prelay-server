use axum::Router;
use prelay_server::{
    app, client_update::ClientUpdateCache, extensions::ExtensionCatalog,
    provider_catalog::ProviderCatalog, AppState,
};

use crate::support::TestStorage;

pub struct TestContext {
    pub app: Router,
    pub storage: TestStorage,
}

pub async fn test_context() -> TestContext {
    test_context_with_catalog(prelay_server::test_support::fixture_catalog()).await
}

pub async fn test_context_with_catalog(provider_catalog: ProviderCatalog) -> TestContext {
    let storage = crate::support::test_storage().await;
    let app = app::router(AppState {
        provider_catalog: std::sync::Arc::new(provider_catalog),
        storage: storage.storage().clone(),
        client: reqwest::Client::new(),
        provider_clients: Default::default(),
        client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
        extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
    })
    .await
    .expect("build application router");
    TestContext { app, storage }
}
