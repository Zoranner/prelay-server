use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use prelay_server::{app, test_support::test_state};
use tower::ServiceExt;

use crate::auth::register;

#[tokio::test]
async fn provider_catalog_is_available_to_authenticated_management_clients() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-catalog", "S-1-5-21-catalog").await;
    let credential = identity["credential"].as_str().expect("credential");

    let request = Request::builder()
        .method("GET")
        .uri("/api/provider-catalog")
        .header("authorization", format!("Bearer {credential}"))
        .body(Body::empty())
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("route request");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response");

    assert_eq!(status, StatusCode::OK);
    let catalog: serde_json::Value = serde_json::from_slice(&body).expect("decode catalog");
    assert!(catalog["language_models"].is_array());
    assert!(catalog["image_generation_models"].is_array());
    assert!(catalog["providers"].is_array());
}
