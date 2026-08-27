use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use prelay_protocol::{
    CreateEndpointRequest, CreateIdentityRequest, CreateProviderRequest, EndpointModelInput,
    ProviderCapabilityOverrides,
};
use serde_json::Value;
use tokio::net::TcpListener;
use tower::ServiceExt;

mod support;
mod test_context;

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

async fn create_endpoint_for(
    app: &axum::Router,
    machine_id: &str,
    account_sid: &str,
    provider_name: &str,
) -> Value {
    create_endpoint_for_url(
        app,
        machine_id,
        account_sid,
        provider_name,
        &format!("https://{provider_name}.example"),
    )
    .await
}

async fn create_endpoint_for_url(
    app: &axum::Router,
    machine_id: &str,
    account_sid: &str,
    provider_name: &str,
    base_url: &str,
) -> Value {
    let credential = valid_credential(&format!("{machine_id}-{account_sid}"));
    let identity = management_post(
        app,
        "/api/identities",
        None,
        serde_json::to_value(CreateIdentityRequest {
            machine_id: machine_id.to_string(),
            account_sid: account_sid.to_string(),
            credential: credential.clone(),
            display_name: None,
        })
        .expect("serialize identity"),
    )
    .await;
    assert!(identity.get("credential").is_none());
    let provider = management_post(
        app,
        "/api/providers",
        Some(&credential),
        serde_json::to_value(CreateProviderRequest {
            name: provider_name.to_string(),
            provider_type: "openai_compatible".to_string(),
            base_url: base_url.to_string(),
            api_key: format!("sk-{provider_name}"),
            capabilities: None,
            models: vec!["upstream-model".to_string()],
        })
        .expect("serialize provider"),
    )
    .await;
    management_post(
        app,
        "/api/endpoints",
        Some(&credential),
        serde_json::to_value(CreateEndpointRequest {
            name: format!("{provider_name} endpoint"),
            protocol: None,
            models: vec![EndpointModelInput {
                model_name: Some("shared-model".to_string()),
                provider_id: provider["id"].as_str().expect("provider id").to_string(),
                upstream_model: "upstream-model".to_string(),
            }],
        })
        .expect("serialize endpoint"),
    )
    .await
}

fn valid_credential(seed: &str) -> String {
    let mut bytes = [0_u8; 32];
    for (index, byte) in seed.bytes().take(bytes.len()).enumerate() {
        bytes[index] = byte;
    }
    URL_SAFE_NO_PAD.encode(bytes)
}

async fn spawn_chat_upstream() -> String {
    async fn handler() -> axum::Json<Value> {
        axum::Json(serde_json::json!({
            "id": "chat_upstream",
            "model": "upstream-model",
            "choices": [{ "message": { "role": "assistant", "content": "hello" } }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 4 }
        }))
    }

    let app = axum::Router::new().route("/chat/completions", axum::routing::post(handler));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let address = listener.local_addr().expect("upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    format!("http://{address}")
}

async fn create_image_endpoint_for_url(
    app: &axum::Router,
    machine_id: &str,
    account_sid: &str,
    provider_name: &str,
    base_url: &str,
) -> Value {
    let credential = valid_credential(&format!("{machine_id}-{account_sid}"));
    let identity = management_post(
        app,
        "/api/identities",
        None,
        serde_json::to_value(CreateIdentityRequest {
            machine_id: machine_id.to_string(),
            account_sid: account_sid.to_string(),
            credential: credential.clone(),
            display_name: None,
        })
        .expect("serialize identity"),
    )
    .await;
    assert!(identity.get("credential").is_none());
    let provider = management_post(
        app,
        "/api/providers",
        Some(&credential),
        serde_json::to_value(CreateProviderRequest {
            name: provider_name.to_string(),
            provider_type: "custom_image".to_string(),
            base_url: base_url.to_string(),
            api_key: format!("sk-{provider_name}"),
            capabilities: Some(ProviderCapabilityOverrides {
                upstream_protocols: Some(vec!["images_generations".to_string()]),
                ..ProviderCapabilityOverrides::default()
            }),
            models: vec!["image-upstream".to_string()],
        })
        .expect("serialize image provider"),
    )
    .await;
    management_post(
        app,
        "/api/endpoints",
        Some(&credential),
        serde_json::to_value(CreateEndpointRequest {
            name: format!("{provider_name} endpoint"),
            protocol: None,
            models: vec![EndpointModelInput {
                model_name: Some("image-public".to_string()),
                provider_id: provider["id"].as_str().expect("provider id").to_string(),
                upstream_model: "image-upstream".to_string(),
            }],
        })
        .expect("serialize image endpoint"),
    )
    .await
}

async fn spawn_identity_image_upstream(provider_name: &'static str) -> (String, Arc<AtomicUsize>) {
    async fn handler(
        State((provider_name, hits)): State<(&'static str, Arc<AtomicUsize>)>,
        axum::Json(payload): axum::Json<Value>,
    ) -> axum::Json<Value> {
        hits.fetch_add(1, Ordering::SeqCst);
        assert_eq!(payload["model"], "image-upstream");
        axum::Json(serde_json::json!({
            "provider": provider_name,
            "data": [{ "url": format!("https://{provider_name}.example/result") }]
        }))
    }

    let hits = Arc::new(AtomicUsize::new(0));
    let app = axum::Router::new()
        .route("/images/generations", axum::routing::post(handler))
        .with_state((provider_name, Arc::clone(&hits)));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind image upstream");
    let address = listener.local_addr().expect("image upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve image upstream");
    });
    (format!("http://{address}"), hits)
}

#[tokio::test]
async fn endpoint_token_resolves_only_its_identity_model_mapping() {
    let context = test_context::test_context().await;
    let app = context.app;
    let endpoint_a = create_endpoint_for(&app, "machine-a", "S-1-5-21-100", "provider-a").await;
    create_endpoint_for(&app, "machine-b", "S-1-5-21-200", "provider-b").await;

    let token = endpoint_a["token"].as_str().expect("endpoint token");
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

#[tokio::test]
async fn image_generation_endpoint_token_uses_only_its_identity_mapping() {
    let context = test_context::test_context().await;
    let app = context.app;
    let (upstream_a, hits_a) = spawn_identity_image_upstream("provider-a").await;
    let (upstream_b, hits_b) = spawn_identity_image_upstream("provider-b").await;
    let endpoint_a =
        create_image_endpoint_for_url(&app, "machine-a", "S-1-5-21-100", "provider-a", &upstream_a)
            .await;
    create_image_endpoint_for_url(&app, "machine-b", "S-1-5-21-200", "provider-b", &upstream_b)
        .await;

    let token = endpoint_a["token"].as_str().expect("endpoint token");
    let (status, response) = request(
        &app,
        Request::builder()
            .method("POST")
            .uri("/v1/images/generations")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "model": "image-public",
                    "prompt": "private prompt"
                })
                .to_string(),
            ))
            .expect("build image request"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["provider"], "provider-a");
    assert_eq!(hits_a.load(Ordering::SeqCst), 1);
    assert_eq!(hits_b.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protocol_request_writes_identity_scoped_log_and_response_session() {
    let context = test_context::test_context().await;
    let app = context.app;
    let upstream = spawn_chat_upstream().await;
    let endpoint_a =
        create_endpoint_for_url(&app, "machine-a", "S-1-5-21-100", "provider-a", &upstream).await;
    let credential_b = valid_credential("machine-b-S-1-5-21-200");
    let identity_b = management_post(
        &app,
        "/api/identities",
        None,
        serde_json::to_value(CreateIdentityRequest {
            machine_id: "machine-b".to_string(),
            account_sid: "S-1-5-21-200".to_string(),
            credential: credential_b.clone(),
            display_name: None,
        })
        .expect("serialize identity B"),
    )
    .await;
    let token = endpoint_a["token"].as_str().expect("endpoint token");
    let (status, response) = request(
        &app,
        Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "model": "shared-model", "input": "hello" }).to_string(),
            ))
            .expect("build protocol request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");

    let access = context
        .storage
        .authenticate_protocol_access(token)
        .await
        .expect("authenticate endpoint token")
        .expect("resolve endpoint access");
    let logs = context
        .storage
        .list_request_logs(&access.identity_id, 10)
        .await
        .expect("load protocol request logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(
        logs[0].endpoint_name.as_deref(),
        Some(access.endpoint_name.as_str())
    );
    let response_id = response["id"].as_str().expect("response id");
    assert!(context
        .storage
        .load_response_session_messages(&access.identity_id, response_id)
        .await
        .expect("load protocol response session")
        .is_some());

    assert!(identity_b.get("credential").is_none());
    let (status, stats) = request(
        &app,
        Request::builder()
            .uri("/api/stats/overview")
            .header("authorization", format!("Bearer {credential_b}"))
            .body(Body::empty())
            .expect("build stats request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["total_requests"], 0);
}
