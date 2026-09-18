use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use prelay_server::{app, test_support::test_state};
use tower::ServiceExt;

use crate::{auth::register, http::request_json};

#[tokio::test]
async fn management_errors_always_carry_a_stable_code() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential = identity["credential"].as_str().expect("credential");

    for (path, token, expected_status, expected_code) in [
        (
            "/api/providers",
            None,
            StatusCode::UNAUTHORIZED,
            "invalid_credential",
        ),
        (
            "/api/catalog/providers/unknown-provider",
            Some(credential),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
    ] {
        let (status, error): (StatusCode, serde_json::Value) =
            request_json(&app, "GET", path, token, None).await;

        assert_eq!(status, expected_status, "{path}");
        assert_eq!(error["error"]["code"], expected_code, "{path}");
        assert!(error["error"]["message"].is_string(), "{path}: {error}");
    }
}

#[tokio::test]
async fn management_rejections_use_the_error_envelope() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential = identity["credential"].as_str().expect("credential");

    for (method, path, content_type, body, expected_status, expected_code) in [
        (
            "GET",
            "/api/does-not-exist",
            None,
            "",
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            "POST",
            "/api/providers",
            Some("application/json"),
            "{ not json",
            StatusCode::BAD_REQUEST,
            "validation_failed",
        ),
        (
            "POST",
            "/api/providers",
            Some("text/plain"),
            "{}",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "validation_failed",
        ),
        (
            "GET",
            "/api/stats/overview?range=bogus",
            None,
            "",
            StatusCode::BAD_REQUEST,
            "validation_failed",
        ),
        (
            "GET",
            "/api/stats/models?scope=bogus",
            None,
            "",
            StatusCode::BAD_REQUEST,
            "validation_failed",
        ),
        (
            "DELETE",
            "/api/identity",
            None,
            "",
            StatusCode::NOT_FOUND,
            "not_found",
        ),
    ] {
        let (status, error) = request_raw(&app, method, path, credential, content_type, body).await;

        assert_eq!(status, expected_status, "{method} {path}");
        assert_eq!(error["error"]["code"], expected_code, "{method} {path}");
        assert!(
            error["error"]["message"].is_string(),
            "{method} {path}: {error}"
        );
    }
}

async fn request_raw(
    app: &axum::Router,
    method: &str,
    path: &str,
    credential: &str,
    content_type: Option<&str>,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {credential}"));
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    let request = builder
        .body(Body::from(body.to_owned()))
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("route request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response");
    (
        status,
        serde_json::from_slice(&bytes).expect("decode json response"),
    )
}
