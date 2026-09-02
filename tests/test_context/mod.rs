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
    let storage = crate::support::test_storage().await;
    let provider_catalog = ProviderCatalog::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("deploy/app/config/catalog")
            .as_path(),
    )
    .expect("load provider catalog");
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
