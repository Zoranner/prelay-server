use axum::{
    body::{to_bytes, Body},
    http::{HeaderMap, Request, StatusCode},
    routing::{get, post},
    Json, Router,
};
use provider_relay_protocol::{
    CreateIdentityRequest, CreateProviderRequest, ModelStatsSummary, ProviderStatsSummary,
    RequestLogSummary, StatsOverview,
};
use provider_relay_server::{app, test_support::test_state};
use serde::de::DeserializeOwned;
use sqlx::SqlitePool;
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

async fn request_status(
    app: &axum::Router,
    method: &str,
    path: &str,
    credential: Option<&str>,
) -> StatusCode {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(credential) = credential {
        builder = builder.header("authorization", format!("Bearer {credential}"));
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("build request"))
        .await
        .expect("route request")
        .status()
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

struct RequestLogSeed<'a> {
    id: &'a str,
    identity_id: &'a str,
    provider_id: &'a str,
    provider_name: &'a str,
    model_requested: &'a str,
    status: &'a str,
    input_tokens: i64,
    output_tokens: i64,
}

async fn seed_request_log(db: &SqlitePool, seed: RequestLogSeed<'_>) {
    sqlx::query(
        "INSERT INTO identity_request_logs (\
            id, identity_id, created_at, protocol_in, protocol_upstream, provider_id, provider_name, \
            model_requested, status, http_status, input_tokens, output_tokens, latency_ms\
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(seed.id)
    .bind(seed.identity_id)
    .bind("2026-08-13T00:00:00Z")
    .bind("chat_completions")
    .bind("openai")
    .bind(seed.provider_id)
    .bind(seed.provider_name)
    .bind(seed.model_requested)
    .bind(seed.status)
    .bind(200_i64)
    .bind(seed.input_tokens)
    .bind(seed.output_tokens)
    .bind(120_i64)
    .execute(db)
    .await
    .expect("seed request log");
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

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/identity/credential/rotate",
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _): (StatusCode, serde_json::Value) =
        request_json(&app, "GET", "/api/providers", Some(credential), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, providers): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/providers", Some(new_credential), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());
}

#[tokio::test]
async fn management_provider_rejects_duplicate_model_names_without_creating_a_provider() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential = identity["credential"].as_str().expect("credential");

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Provider A",
            "provider_type": "openai_compatible",
            "base_url": "https://provider-a.example",
            "api_key": "sk-a",
            "models": ["model-a", " model-a "]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "validation_failed");

    let (status, providers): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/providers", Some(credential), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());
}

#[tokio::test]
async fn management_interface_rejects_duplicate_model_names_without_creating_an_interface() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential = identity["credential"].as_str().expect("credential");
    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Provider A",
            "provider_type": "openai_compatible",
            "base_url": "https://provider-a.example",
            "api_key": "sk-a",
            "models": ["model-a"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/interfaces",
        Some(credential),
        Some(serde_json::json!({
            "name": "Interface A",
            "models": [
                {
                    "provider_id": provider_id,
                    "upstream_model": "model-a",
                    "model_name": "public-model"
                },
                {
                    "provider_id": provider_id,
                    "upstream_model": "model-a",
                    "model_name": " public-model "
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "validation_failed");

    let (status, interfaces): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/interfaces", Some(credential), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(interfaces.is_empty());
}

#[tokio::test]
async fn management_credential_cannot_read_mutate_or_delete_another_identity_interface() {
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
        "GET",
        &format!("/api/interfaces/{interface_id}"),
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/interfaces/{interface_id}"),
        Some(credential_b),
        Some(serde_json::json!({ "name": "Interface B" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert_eq!(
        request_status(
            &app,
            "DELETE",
            &format!("/api/interfaces/{interface_id}"),
            Some(credential_b),
        )
        .await,
        StatusCode::NOT_FOUND
    );

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/interfaces/{interface_id}/regenerate-token"),
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, interface): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/interfaces/{interface_id}"),
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(interface["name"], "Interface A");
}

#[tokio::test]
async fn management_stats_only_return_the_current_identity_request_data() {
    let state = test_state().await;
    let db = state.db.clone();
    let app = app::router(state).await.expect("build app");

    let identity_a = register(&app, "machine-a", "S-1-5-21-100").await;
    let identity_a_id = identity_a["identity_id"].as_str().expect("identity A id");
    let credential_a = identity_a["credential"].as_str().expect("credential A");
    let (status, provider_a): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential_a),
        Some(
            serde_json::to_value(CreateProviderRequest {
                name: "Provider A".to_string(),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider-a.example".to_string(),
                api_key: "sk-a".to_string(),
                capabilities: None,
                models: vec!["model-a".to_string()],
            })
            .expect("serialize provider A"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_a_id = provider_a["id"].as_str().expect("provider A id");

    let identity_b = register(&app, "machine-b", "S-1-5-21-200").await;
    let identity_b_id = identity_b["identity_id"].as_str().expect("identity B id");
    let credential_b = identity_b["credential"].as_str().expect("credential B");
    let (status, provider_b): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential_b),
        Some(
            serde_json::to_value(CreateProviderRequest {
                name: "Provider B".to_string(),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider-b.example".to_string(),
                api_key: "sk-b".to_string(),
                capabilities: None,
                models: vec!["model-b".to_string()],
            })
            .expect("serialize provider B"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_b_id = provider_b["id"].as_str().expect("provider B id");

    seed_request_log(
        &db,
        RequestLogSeed {
            id: "request-a",
            identity_id: identity_a_id,
            provider_id: provider_a_id,
            provider_name: "Provider A",
            model_requested: "model-a",
            status: "success",
            input_tokens: 3,
            output_tokens: 4,
        },
    )
    .await;
    seed_request_log(
        &db,
        RequestLogSeed {
            id: "request-b",
            identity_id: identity_b_id,
            provider_id: provider_b_id,
            provider_name: "Provider B",
            model_requested: "model-b",
            status: "failed",
            input_tokens: 5,
            output_tokens: 6,
        },
    )
    .await;

    let (status, overview_a): (StatusCode, StatsOverview) =
        request_json(&app, "GET", "/api/stats/overview", Some(credential_a), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_a.total_requests, 1);
    assert_eq!(overview_a.successful_requests, 1);
    assert_eq!(overview_a.failed_requests, 0);
    assert_eq!(overview_a.input_tokens, 3);
    assert_eq!(overview_a.output_tokens, 4);

    let (status, requests_a): (StatusCode, Vec<RequestLogSummary>) =
        request_json(&app, "GET", "/api/stats/requests", Some(credential_a), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(requests_a.len(), 1);
    assert_eq!(requests_a[0].id, "request-a");
    assert_eq!(requests_a[0].provider_name.as_deref(), Some("Provider A"));

    let (status, models_a): (StatusCode, Vec<ModelStatsSummary>) =
        request_json(&app, "GET", "/api/stats/models", Some(credential_a), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models_a.len(), 1);
    assert_eq!(models_a[0].model_requested.as_deref(), Some("model-a"));
    assert_eq!(models_a[0].total_requests, 1);
    assert_eq!(models_a[0].input_tokens, 3);
    assert_eq!(models_a[0].output_tokens, 4);

    let (status, providers_a): (StatusCode, Vec<ProviderStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/providers",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers_a.len(), 1);
    assert_eq!(providers_a[0].provider_id.as_deref(), Some(provider_a_id));
    assert_eq!(providers_a[0].provider_name.as_deref(), Some("Provider A"));
    assert_eq!(providers_a[0].total_requests, 1);

    let (status, overview_b): (StatusCode, StatsOverview) =
        request_json(&app, "GET", "/api/stats/overview", Some(credential_b), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_b.total_requests, 1);
    assert_eq!(overview_b.successful_requests, 0);
    assert_eq!(overview_b.failed_requests, 1);
    assert_eq!(overview_b.input_tokens, 5);
    assert_eq!(overview_b.output_tokens, 6);

    let (status, requests_b): (StatusCode, Vec<RequestLogSummary>) =
        request_json(&app, "GET", "/api/stats/requests", Some(credential_b), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(requests_b.len(), 1);
    assert_eq!(requests_b[0].id, "request-b");
    assert_eq!(requests_b[0].provider_name.as_deref(), Some("Provider B"));

    let (status, models_b): (StatusCode, Vec<ModelStatsSummary>) =
        request_json(&app, "GET", "/api/stats/models", Some(credential_b), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models_b.len(), 1);
    assert_eq!(models_b[0].model_requested.as_deref(), Some("model-b"));
    assert_eq!(models_b[0].total_requests, 1);
    assert_eq!(models_b[0].input_tokens, 5);
    assert_eq!(models_b[0].output_tokens, 6);

    let (status, providers_b): (StatusCode, Vec<ProviderStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/providers",
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers_b.len(), 1);
    assert_eq!(providers_b[0].provider_id.as_deref(), Some(provider_b_id));
    assert_eq!(providers_b[0].provider_name.as_deref(), Some("Provider B"));
    assert_eq!(providers_b[0].total_requests, 1);
}

#[tokio::test]
async fn management_credential_deletes_own_interface_with_model_mapping() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity_a = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential_a = identity_a["credential"].as_str().expect("credential A");
    let provider_request = CreateProviderRequest {
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
        Some(serde_json::to_value(provider_request).expect("serialize provider request")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, interface): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/interfaces",
        Some(credential_a),
        Some(serde_json::json!({
            "name": "Interface A",
            "models": [{
                "model_name": "public-model",
                "provider_id": provider_id,
                "upstream_model": "model-a"
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        interface["models"]
            .as_array()
            .expect("interface models")
            .len(),
        1
    );
    let interface_id = interface["id"].as_str().expect("interface id");

    let identity_b = register(&app, "machine-b", "S-1-5-21-200").await;
    let credential_b = identity_b["credential"].as_str().expect("credential B");
    assert_eq!(
        request_status(
            &app,
            "DELETE",
            &format!("/api/interfaces/{interface_id}"),
            Some(credential_b),
        )
        .await,
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        request_status(
            &app,
            "DELETE",
            &format!("/api/interfaces/{interface_id}"),
            Some(credential_a),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/interfaces/{interface_id}"),
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, interfaces): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/interfaces", Some(credential_a), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(interfaces.is_empty());
}

async fn spawn_provider_actions_upstream(expected_api_key: &'static str) -> String {
    async fn models(headers: HeaderMap) -> Json<serde_json::Value> {
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-provider-action-secret")
        );
        Json(serde_json::json!({
            "data": [{ "id": "discovered-model" }]
        }))
    }

    async fn chat(headers: HeaderMap) -> (StatusCode, &'static str) {
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-provider-action-secret")
        );
        (StatusCode::OK, "data: {\"choices\":[]}\n\n")
    }

    assert_eq!(expected_api_key, "sk-provider-action-secret");
    let upstream = Router::new()
        .route("/models", get(models))
        .route("/chat/completions", post(chat));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let address = listener.local_addr().expect("read upstream address");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });
    format!("http://{address}")
}

#[tokio::test]
async fn management_provider_actions_use_only_the_current_identity_and_keep_key_private() {
    let app = app::router(test_state().await).await.expect("build app");
    let credential_a = register(&app, "machine-actions-a", "S-1-5-21-610").await["credential"]
        .as_str()
        .expect("credential A")
        .to_string();
    let base_url = spawn_provider_actions_upstream("sk-provider-action-secret").await;
    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(&credential_a),
        Some(serde_json::json!({
            "name": "Action Provider",
            "provider_type": "openai_compatible",
            "base_url": base_url,
            "api_key": "sk-provider-action-secret",
            "models": ["saved-model"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");
    assert!(!provider.to_string().contains("sk-provider-action-secret"));

    assert_eq!(
        request_status(
            &app,
            "POST",
            &format!("/api/providers/{provider_id}/ping"),
            Some(&credential_a),
        )
        .await,
        StatusCode::OK
    );

    let (status, ping): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/providers/{provider_id}/ping"),
        Some(&credential_a),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ping["ok"], true);
    assert!(!ping.to_string().contains("sk-provider-action-secret"));

    let (status, discovered): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/providers/{provider_id}/discover-models"),
        Some(&credential_a),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(discovered["ok"], true);
    assert_eq!(
        discovered["models"],
        serde_json::json!(["discovered-model"])
    );
    assert!(!discovered.to_string().contains("sk-provider-action-secret"));

    let (status, protocol): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/providers/{provider_id}/test-protocol"),
        Some(&credential_a),
        Some(serde_json::json!({ "protocol": "openai", "model": "discovered-model" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(protocol["ok"], true);
    assert_eq!(protocol["protocol"], "openai");
    assert!(!protocol.to_string().contains("sk-provider-action-secret"));

    let credential_b = register(&app, "machine-actions-b", "S-1-5-21-620").await["credential"]
        .as_str()
        .expect("credential B")
        .to_string();
    for action in ["ping", "discover-models", "test-protocol"] {
        let (status, response): (StatusCode, serde_json::Value) = request_json(
            &app,
            "POST",
            &format!("/api/providers/{provider_id}/{action}"),
            Some(&credential_b),
            Some(serde_json::json!({ "protocol": "openai", "model": "discovered-model" })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{action}");
        assert!(!response.to_string().contains("sk-provider-action-secret"));
    }
}
