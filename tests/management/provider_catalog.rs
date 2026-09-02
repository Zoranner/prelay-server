use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use prelay_server::{app, test_support::test_state};
use tower::ServiceExt;

use crate::auth::register;

#[tokio::test]
async fn catalog_lists_providers_and_model_categories() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-catalog", "S-1-5-21-catalog").await;
    let credential = identity["credential"].as_str().expect("credential");

    let request = Request::builder()
        .method("GET")
        .uri("/api/catalog/providers")
        .header("authorization", format!("Bearer {credential}"))
        .body(Body::empty())
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("route request");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response");

    assert_eq!(status, StatusCode::OK);
    let providers: serde_json::Value = serde_json::from_slice(&body).expect("decode providers");
    assert!(providers.as_array().is_some_and(|items| !items.is_empty()));

    for path in [
        "/api/catalog/models/language",
        "/api/catalog/models/image-generation",
    ] {
        let request = Request::builder()
            .method("GET")
            .uri(path)
            .header("authorization", format!("Bearer {credential}"))
            .body(Body::empty())
            .expect("build request");
        let response = app.clone().oneshot(request).await.expect("route request");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response");
        let models: serde_json::Value = serde_json::from_slice(&body).expect("decode models");
        assert!(models.as_array().is_some_and(|items| !items.is_empty()));
    }
}

#[tokio::test]
async fn catalog_returns_provider_and_model_details() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-catalog-detail", "S-1-5-21-catalog-detail").await;
    let credential = identity["credential"].as_str().expect("credential");
    for path in [
        "/api/catalog/providers/gotoken",
        "/api/catalog/models/language/gpt-5.6-sol",
        "/api/catalog/models/image-generation/gpt-image-1",
    ] {
        let request = Request::builder()
            .method("GET")
            .uri(path)
            .header("authorization", format!("Bearer {credential}"))
            .body(Body::empty())
            .expect("build request");
        let response = app.clone().oneshot(request).await.expect("route request");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("decode detail");
        assert!(value["id"].is_string());
    }
}
