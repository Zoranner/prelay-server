use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use prelay_protocol::{
    CreateEndpointRequest, CreateIdentityRequest, CreateProviderRequest, EndpointModelInput,
};
use prelay_server::{app, test_support::test_state};
use serde_json::Value;
use sqlx::Row;
use tokio::net::TcpListener;
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

#[tokio::test]
async fn endpoint_token_resolves_only_its_identity_model_mapping() {
    let app = app::router(test_state().await).await.expect("build app");
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
async fn protocol_request_writes_identity_scoped_log_and_response_session() {
    let state = test_state().await;
    let db = state.db.clone();
    let app = app::router(state).await.expect("build app");
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

    let identity_id = sqlx::query_scalar::<_, String>(
        "SELECT identity_id FROM identity_endpoint_configs WHERE token = ?",
    )
    .bind(token)
    .fetch_one(&db)
    .await
    .expect("load protocol identity");
    let log_identity = sqlx::query("SELECT identity_id FROM identity_request_logs")
        .fetch_one(&db)
        .await
        .expect("load identity log")
        .get::<String, _>("identity_id");
    let session_identity = sqlx::query("SELECT identity_id FROM identity_response_sessions")
        .fetch_one(&db)
        .await
        .expect("load identity session")
        .get::<String, _>("identity_id");
    assert_eq!(log_identity, identity_id);
    assert_eq!(session_identity, identity_id);

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
    assert!(response["id"].as_str().is_some());
}
