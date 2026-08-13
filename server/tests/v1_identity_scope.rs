use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use provider_relay_protocol::{
    CreateIdentityRequest, CreateInterfaceRequest, CreateProviderRequest, InterfaceModelInput,
};
use provider_relay_server::{app, test_support::test_state};
use serde_json::Value;
use tower::ServiceExt;

async fn request(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("route request");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response");
    (
        status,
        serde_json::from_slice(&body).expect("decode JSON response"),
    )
}

async fn management_post(
    app: &axum::Router,
    path: &str,
    credential: Option<&str>,
    body: Value,
) -> Value {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(credential) = credential {
        builder = builder.header("authorization", format!("Bearer {credential}"));
    }
    let (status, response) = request(
        app,
        builder
            .body(Body::from(body.to_string()))
            .expect("build management request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    response
}

async fn create_interface_for(
    app: &axum::Router,
    machine_id: &str,
    account_sid: &str,
    provider_name: &str,
) -> Value {
    let identity = management_post(
        app,
        "/api/identities",
        None,
        serde_json::to_value(CreateIdentityRequest {
            machine_id: machine_id.to_string(),
            account_sid: account_sid.to_string(),
        })
        .expect("serialize identity"),
    )
    .await;
    let credential = identity["credential"]
        .as_str()
        .expect("identity credential");
    let provider = management_post(
        app,
        "/api/providers",
        Some(credential),
        serde_json::to_value(CreateProviderRequest {
            name: provider_name.to_string(),
            provider_type: "openai_compatible".to_string(),
            base_url: format!("https://{provider_name}.example"),
            api_key: format!("sk-{provider_name}"),
            capabilities: None,
            models: vec!["upstream-model".to_string()],
        })
        .expect("serialize provider"),
    )
    .await;
    management_post(
        app,
        "/api/interfaces",
        Some(credential),
        serde_json::to_value(CreateInterfaceRequest {
            name: format!("{provider_name} interface"),
            protocol: None,
            models: vec![InterfaceModelInput {
                model_name: Some("shared-model".to_string()),
                provider_id: provider["id"].as_str().expect("provider id").to_string(),
                upstream_model: "upstream-model".to_string(),
            }],
        })
        .expect("serialize interface"),
    )
    .await
}

#[tokio::test]
async fn interface_token_resolves_only_its_identity_model_mapping() {
    let app = app::router(test_state().await).await.expect("build app");
    let interface_a = create_interface_for(&app, "machine-a", "S-1-5-21-100", "provider-a").await;
    create_interface_for(&app, "machine-b", "S-1-5-21-200", "provider-b").await;

    let token = interface_a["token"].as_str().expect("interface token");
    let (status, response) = request(
        &app,
        Request::builder()
            .uri("/v1/models")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .expect("build v1 request"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["data"][0]["id"], "shared-model");
    assert_eq!(response["data"][0]["provider_name"], "provider-a");
    assert_eq!(response["data"].as_array().expect("models").len(), 1);
}
