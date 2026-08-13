use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use provider_relay_server::{app, test_support::test_state};
use tower::ServiceExt;

async fn status(app: &axum::Router, path: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request")
        .status()
}

#[tokio::test]
async fn v1_models_route_is_registered_without_static_or_proxy_fallback() {
    let app = app::router(test_state().await).await.expect("build app");

    assert_eq!(status(&app, "/v1/models").await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(&app, "/proxy").await, StatusCode::NOT_FOUND);
    assert_eq!(status(&app, "/").await, StatusCode::NOT_FOUND);
}
