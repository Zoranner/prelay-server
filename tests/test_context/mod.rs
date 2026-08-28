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
    let app = app::router(AppState {
        storage: storage.storage().clone(),
        client: reqwest::Client::new(),
        client_update: ClientUpdateCache::unavailable(reqwest::Client::new()),
        extensions: ExtensionCatalog::unavailable(reqwest::Client::new()),
    })
    .await
    .expect("build application router");
    TestContext { app, storage }
}
