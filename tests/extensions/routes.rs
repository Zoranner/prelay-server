use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use prelay_protocol::CreateIdentityRequest;
use prelay_server::{app, test_support::test_state};
use tower::ServiceExt;

#[tokio::test]
async fn extension_catalog_routes_require_a_device_credential() {
    let app = app::router(test_state().await).await.expect("build app");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/extensions/rules")
                .body(Body::empty())
                .expect("build anonymous request"),
        )
        .await
        .expect("route request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let credential = valid_credential();
    register(&app, &credential).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/extensions/rules")
                .header("authorization", format!("Bearer {credential}"))
                .body(Body::empty())
                .expect("build authenticated request"),
        )
        .await
        .expect("route request");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read error response");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({
            "error": {
                "code": "extension_catalog_unavailable",
                "message": "扩展目录暂不可用"
            }
        })
    );
}

async fn register(app: &axum::Router, credential: &str) {
    let request = CreateIdentityRequest {
        machine_id: "extension-machine".to_string(),
        account_sid: "S-1-5-21-100".to_string(),
        credential: credential.to_string(),
        display_name: None,
    };
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/identities")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .expect("build identity registration request"),
        )
        .await
        .expect("route request");
    assert_eq!(response.status(), StatusCode::CREATED);
}

fn valid_credential() -> String {
    URL_SAFE_NO_PAD.encode([b'x'; 32])
}
