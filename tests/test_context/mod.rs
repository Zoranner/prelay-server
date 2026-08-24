use axum::Router;
use prelay_server::{app, AppState};

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
    })
    .await
    .expect("build application router");
    TestContext { app, storage }
}
