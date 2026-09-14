use axum::Router;
use prelay_server::{
    app, client_update::ClientUpdateCache, extensions::ExtensionCatalog, AppState,
};

use crate::support::TestStorage;

pub struct TestContext {
    pub app: Router,
    pub storage: TestStorage,
}

pub async fn test_context() -> TestContext {
    let storage = crate::support::test_storage().await;
    let provider_catalog = prelay_server::test_support::fixture_catalog();
    let app = app::router(AppState {
        provider_catalog: std::sync::Arc::new(provider_catalog),
        storage: storage.storage().clone(),
        client: reqwest::Client::new(),
        client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
        extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
    })
    .await
    .expect("build application router");
    TestContext { app, storage }
}
