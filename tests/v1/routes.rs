use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use prelay_server::{bootstrap as app, test_support::test_state};
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

async fn post_status(app: &axum::Router, path: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"image-upstream","prompt":"private prompt"}"#,
                ))
                .expect("build request"),
        )
        .await
        .expect("route request")
        .status()
}

async fn response_body(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read response");
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, body)
}

#[tokio::test]
async fn v1_models_route_is_registered_without_static_or_proxy_fallback() {
    let app = app::router(test_state().await).await.expect("build app");

    assert_eq!(status(&app, "/v1/models").await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        post_status(&app, "/v1/images/generations").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        post_status(&app, "/v1/messages").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(status(&app, "/proxy").await, StatusCode::NOT_FOUND);
    assert_eq!(status(&app, "/").await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn v1_errors_use_the_openai_envelope() {
    let app = app::router(test_state().await).await.expect("build app");

    for (path, expected_status, expected_code) in [
        ("/v1/models", StatusCode::UNAUTHORIZED, "invalid_api_key"),
        ("/v1/messages", StatusCode::NOT_FOUND, "not_found"),
    ] {
        let (status, body) = response_body(&app, path).await;

        assert_eq!(status, expected_status, "{path}");
        assert_eq!(body["error"]["code"], expected_code, "{path}: {body}");
        assert!(body["error"]["type"].is_string(), "{path}: {body}");
        assert!(body["error"]["message"].is_string(), "{path}: {body}");
        assert!(body["error"].get("param").is_some(), "{path}: {body}");
    }
}
