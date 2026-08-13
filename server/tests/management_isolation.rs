use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use provider_relay_protocol::{CreateIdentityRequest, CreateProviderRequest};
use provider_relay_server::{app, test_support::test_state};
use serde::de::DeserializeOwned;
use tower::ServiceExt;

async fn request_json<T: DeserializeOwned>(
    app: &axum::Router,
    method: &str,
    path: &str,
    credential: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, T) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(credential) = credential {
        builder = builder.header("authorization", format!("Bearer {credential}"));
    }
    let request = builder
        .header("content-type", "application/json")
        .body(Body::from(
            body.map(|value| value.to_string()).unwrap_or_default(),
        ))
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

async fn register(app: &axum::Router, machine_id: &str, account_sid: &str) -> serde_json::Value {
    let request = CreateIdentityRequest {
        machine_id: machine_id.to_string(),
        account_sid: account_sid.to_string(),
    };
    let (status, response) = request_json(
        app,
        "POST",
        "/api/identities",
        None,
        Some(serde_json::to_value(request).expect("serialize identity request")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    response
}

#[tokio::test]
async fn management_credential_cannot_read_or_mutate_another_identity_provider() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity_a = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential_a = identity_a["credential"].as_str().expect("credential A");
    let request = CreateProviderRequest {
        name: "Provider A".to_string(),
        provider_type: "openai_compatible".to_string(),
        base_url: "https://provider-a.example".to_string(),
        api_key: "sk-a".to_string(),
        capabilities: None,
        models: vec!["model-a".to_string()],
    };
    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential_a),
        Some(serde_json::to_value(request).expect("serialize provider request")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_a = provider["id"].as_str().expect("provider id");

    let identity_b = register(&app, "machine-b", "S-1-5-21-200").await;
    let credential_b = identity_b["credential"].as_str().expect("credential B");

    let (status, providers): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/providers", Some(credential_b), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "DELETE",
        &format!("/api/providers/{provider_a}"),
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_a}"),
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(provider["name"], "Provider A");
}

#[tokio::test]
async fn management_credential_rotation_invalidates_the_previous_credential() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential = identity["credential"].as_str().expect("credential");

    let (status, rotated): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/identity/credential/rotate",
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let new_credential = rotated["credential"].as_str().expect("new credential");
    assert_ne!(credential, new_credential);

    let (status, _): (StatusCode, serde_json::Value) =
        request_json(&app, "GET", "/api/providers", Some(credential), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, providers): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/providers", Some(new_credential), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());
}

#[tokio::test]
async fn management_credential_cannot_read_another_identity_interface() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity_a = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential_a = identity_a["credential"].as_str().expect("credential A");
    let (status, interface): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/interfaces",
        Some(credential_a),
        Some(serde_json::json!({ "name": "Interface A", "models": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let interface_id = interface["id"].as_str().expect("interface id");

    let identity_b = register(&app, "machine-b", "S-1-5-21-200").await;
    let credential_b = identity_b["credential"].as_str().expect("credential B");
    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/interfaces/{interface_id}/regenerate-token"),
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
