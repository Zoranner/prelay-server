use axum::{
    body::{to_bytes, Body},
    http::{HeaderMap, Request, StatusCode},
    routing::{get, head, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use prelay_protocol::{
    CreateIdentityRequest, CreateProviderRequest, ModelStatsSummary, ProviderStatsSummary,
    RequestLogSummary, StatsOverview,
};
use prelay_server::{app, test_support::test_state};
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
    let credential = valid_credential(&format!("{machine_id}-{account_sid}"));
    let request = CreateIdentityRequest {
        machine_id: machine_id.to_string(),
        account_sid: account_sid.to_string(),
        credential: credential.clone(),
    };
    let (status, mut response): (StatusCode, serde_json::Value) = request_json(
        app,
        "POST",
        "/api/identities",
        None,
        Some(serde_json::to_value(request).expect("serialize identity request")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(response.get("credential").is_none());
    response["credential"] = credential.into();
    response
}

fn valid_credential(seed: &str) -> String {
    let mut bytes = [0_u8; 32];
    for (index, byte) in seed.bytes().take(bytes.len()).enumerate() {
        bytes[index] = byte;
    }
    URL_SAFE_NO_PAD.encode(bytes)
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
            model_requested, status, http_status, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms\
        ) VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(seed.id)
    .bind(seed.identity_id)
    .bind("chat_completions")
    .bind("openai")
    .bind(seed.provider_id)
    .bind(seed.provider_name)
    .bind(seed.model_requested)
    .bind(seed.status)
    .bind(200_i64)
    .bind(seed.input_tokens)
    .bind(seed.output_tokens)
    .bind(1_i64)
    .bind(2_i64)
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
    assert_eq!(
        provider["upstream_protocols"],
        serde_json::json!(["openai"])
    );
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
        Some(serde_json::json!({ "new_credential": valid_credential("rotated") })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rotated["rotated"], true);
    assert!(rotated.get("credential").is_none());
    let new_credential = valid_credential("rotated");

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/identity/credential/rotate",
        Some(credential),
        Some(serde_json::json!({ "new_credential": valid_credential("rotated-again") })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _): (StatusCode, serde_json::Value) =
        request_json(&app, "GET", "/api/providers", Some(credential), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, providers): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/providers", Some(&new_credential), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());
}

#[tokio::test]
async fn management_identity_registration_rejects_blank_or_short_credentials() {
    let app = app::router(test_state().await).await.expect("build app");

    for credential in ["", "credential-too-short"] {
        let request = CreateIdentityRequest {
            machine_id: format!("machine-{credential}"),
            account_sid: "S-1-5-21-100".to_string(),
            credential: credential.to_string(),
        };
        let (status, error): (StatusCode, serde_json::Value) = request_json(
            &app,
            "POST",
            "/api/identities",
            None,
            Some(serde_json::to_value(request).expect("serialize identity request")),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error["error"]["code"], "validation_failed");
    }
}

#[tokio::test]
async fn management_credential_rotation_rejects_blank_or_short_credentials() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential = identity["credential"].as_str().expect("credential");

    for new_credential in ["", "credential-too-short"] {
        let (status, error): (StatusCode, serde_json::Value) = request_json(
            &app,
            "POST",
            "/api/identity/credential/rotate",
            Some(credential),
            Some(serde_json::json!({ "new_credential": new_credential })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error["error"]["code"], "validation_failed");
    }
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
async fn management_endpoint_rejects_duplicate_model_names_without_creating_an_interface() {
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
        "/api/endpoints",
        Some(credential),
        Some(serde_json::json!({
            "name": "Endpoint A",
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

    let (status, endpoints): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/endpoints", Some(credential), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(endpoints.is_empty());
}

#[tokio::test]
async fn management_credential_cannot_read_mutate_or_delete_another_identity_interface() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity_a = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential_a = identity_a["credential"].as_str().expect("credential A");
    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/endpoints",
        Some(credential_a),
        Some(serde_json::json!({ "name": "Endpoint A", "models": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let endpoint_id = endpoint["id"].as_str().expect("endpoint id");

    let identity_b = register(&app, "machine-b", "S-1-5-21-200").await;
    let credential_b = identity_b["credential"].as_str().expect("credential B");

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/endpoints/{endpoint_id}"),
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/endpoints/{endpoint_id}"),
        Some(credential_b),
        Some(serde_json::json!({ "name": "Endpoint B" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert_eq!(
        request_status(
            &app,
            "DELETE",
            &format!("/api/endpoints/{endpoint_id}"),
            Some(credential_b),
        )
        .await,
        StatusCode::NOT_FOUND
    );

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/endpoints/{endpoint_id}/regenerate-token"),
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/endpoints/{endpoint_id}"),
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(endpoint["name"], "Endpoint A");
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
            id: "request-a-last-year",
            identity_id: identity_a_id,
            provider_id: provider_a_id,
            provider_name: "Provider A",
            model_requested: "model-a",
            status: "success",
            input_tokens: 4,
            output_tokens: 5,
        },
    )
    .await;
    sqlx::query(
        "UPDATE identity_request_logs SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 year') WHERE id = ?",
    )
    .bind("request-a-last-year")
    .execute(&db)
    .await
    .expect("move historical request");
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

    let (status, overview_a): (StatusCode, StatsOverview) = request_json(
        &app,
        "GET",
        "/api/stats/overview?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_a.total_requests, 1);
    assert_eq!(overview_a.successful_requests, 1);
    assert_eq!(overview_a.failed_requests, 0);
    assert_eq!(overview_a.input_tokens, 3);
    assert_eq!(overview_a.output_tokens, 4);
    assert_eq!(overview_a.cache_read_tokens, 1);
    assert_eq!(overview_a.cache_write_tokens, 2);
    assert_eq!(overview_a.average_latency_ms, Some(120));

    let (status, timeline_a): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(timeline_a.len(), 24);
    assert_eq!(
        timeline_a
            .iter()
            .map(|point| point["input_tokens"].as_i64().expect("input tokens"))
            .sum::<i64>(),
        3
    );

    let (status, requests_a): (StatusCode, Vec<RequestLogSummary>) =
        request_json(&app, "GET", "/api/stats/requests", Some(credential_a), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(requests_a.len(), 2);
    assert_eq!(requests_a[0].id, "request-a");
    assert_eq!(requests_a[0].provider_name.as_deref(), Some("Provider A"));

    let (status, models_a): (StatusCode, Vec<ModelStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/models?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models_a.len(), 1);
    assert_eq!(models_a[0].model_requested.as_deref(), Some("model-a"));
    assert_eq!(models_a[0].total_requests, 1);
    assert_eq!(models_a[0].input_tokens, 3);
    assert_eq!(models_a[0].output_tokens, 4);

    let (status, providers_a): (StatusCode, Vec<ProviderStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/providers?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers_a.len(), 1);
    assert_eq!(providers_a[0].provider_id.as_deref(), Some(provider_a_id));
    assert_eq!(providers_a[0].provider_name.as_deref(), Some("Provider A"));
    assert_eq!(providers_a[0].total_requests, 1);

    let (status, overview_a_all): (StatusCode, StatsOverview) = request_json(
        &app,
        "GET",
        "/api/stats/overview?range=all",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_a_all.total_requests, 2);
    assert_eq!(overview_a_all.input_tokens, 7);
    assert_eq!(overview_a_all.output_tokens, 9);

    let (status, timeline_a_all): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=all",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(timeline_a_all.len() >= 13);
    assert_eq!(
        timeline_a_all
            .iter()
            .map(|point| point["input_tokens"].as_i64().expect("input tokens"))
            .sum::<i64>(),
        7
    );

    let (status, overview_b): (StatusCode, StatsOverview) =
        request_json(&app, "GET", "/api/stats/overview", Some(credential_b), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_b.total_requests, 1);
    assert_eq!(overview_b.successful_requests, 0);
    assert_eq!(overview_b.failed_requests, 1);
    assert_eq!(overview_b.input_tokens, 5);
    assert_eq!(overview_b.output_tokens, 6);
    assert_eq!(overview_b.cache_read_tokens, 1);
    assert_eq!(overview_b.cache_write_tokens, 2);
    assert_eq!(overview_b.average_latency_ms, Some(120));

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
async fn management_credential_deletes_own_endpoint_with_model_mapping() {
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

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/endpoints",
        Some(credential_a),
        Some(serde_json::json!({
            "name": "Endpoint A",
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
        endpoint["models"]
            .as_array()
            .expect("endpoint models")
            .len(),
        1
    );
    let endpoint_id = endpoint["id"].as_str().expect("endpoint id");

    let identity_b = register(&app, "machine-b", "S-1-5-21-200").await;
    let credential_b = identity_b["credential"].as_str().expect("credential B");
    assert_eq!(
        request_status(
            &app,
            "DELETE",
            &format!("/api/endpoints/{endpoint_id}"),
            Some(credential_b),
        )
        .await,
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        request_status(
            &app,
            "DELETE",
            &format!("/api/endpoints/{endpoint_id}"),
            Some(credential_a),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/endpoints/{endpoint_id}"),
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, endpoints): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/endpoints", Some(credential_a), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(endpoints.is_empty());
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
async fn management_provider_actions_do_not_persist_the_supplied_key() {
    let app = app::router(test_state().await).await.expect("build app");
    let credential = register(&app, "machine-transient", "S-1-5-21-615").await["credential"]
        .as_str()
        .expect("credential")
        .to_string();
    let base_url = spawn_provider_actions_upstream("sk-provider-action-secret").await;
    let input = serde_json::json!({
        "provider_type": "openai_compatible",
        "base_url": base_url,
        "api_key": "sk-provider-action-secret",
        "protocol": "openai",
        "model": "discovered-model"
    });

    let (status, discovered): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers/discover-models",
        Some(&credential),
        Some(input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        discovered["models"],
        serde_json::json!(["discovered-model"])
    );
    assert!(!discovered.to_string().contains("sk-provider-action-secret"));

    let (status, tested): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers/test-protocol",
        Some(&credential),
        Some(input),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tested["ok"], true);
    assert!(!tested.to_string().contains("sk-provider-action-secret"));

    let (status, providers): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/providers", Some(&credential), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());
}

#[tokio::test]
async fn management_provider_response_exposes_the_key_only_to_its_current_identity() {
    let app = app::router(test_state().await).await.expect("build app");
    let credential_a = register(&app, "machine-key-a", "S-1-5-21-617").await["credential"]
        .as_str()
        .expect("credential A")
        .to_string();
    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(&credential_a),
        Some(serde_json::json!({
            "name": "Provider With Visible Key",
            "provider_type": "openai_compatible",
            "base_url": "https://provider.example",
            "api_key": "sk-visible-to-owner-only",
            "models": ["model-a"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(provider["api_key"], "sk-visible-to-owner-only");

    let credential_b = register(&app, "machine-key-b", "S-1-5-21-618").await["credential"]
        .as_str()
        .expect("credential B")
        .to_string();
    let provider_id = provider["id"].as_str().expect("provider id");
    assert_eq!(
        request_status(
            &app,
            "GET",
            &format!("/api/providers/{provider_id}"),
            Some(&credential_b),
        )
        .await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn management_saved_provider_ping_checks_reachability_without_a_provider_key() {
    async fn ping(headers: HeaderMap) -> StatusCode {
        assert!(headers.get("authorization").is_none());
        StatusCode::NO_CONTENT
    }

    let upstream = Router::new().route("/", head(ping));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let address = listener.local_addr().expect("read upstream address");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });

    let app = app::router(test_state().await).await.expect("build app");
    let credential = register(&app, "machine-saved-actions", "S-1-5-21-616").await["credential"]
        .as_str()
        .expect("credential")
        .to_string();
    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(&credential),
        Some(serde_json::json!({
            "name": "Saved Provider",
            "provider_type": "openai_compatible",
            "base_url": format!("http://{address}"),
            "api_key": "sk-provider-action-secret",
            "models": ["saved-model"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, ping): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/providers/{provider_id}/ping"),
        Some(&credential),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ping["ok"], true);
    assert!(!ping.to_string().contains("sk-provider-action-secret"));
}

#[tokio::test]
async fn management_provider_operations_return_success_envelopes_for_upstream_network_failures() {
    let app = app::router(test_state().await).await.expect("build app");
    let credential = register(&app, "machine-upstream-failure", "S-1-5-21-650").await["credential"]
        .as_str()
        .expect("credential")
        .to_string();
    for (path, body, expected_error_prefixes) in [
        (
            "/api/providers/discover-models",
            serde_json::json!({
                "provider_type": "openai_compatible",
                "base_url": "http://127.0.0.1:0",
                "api_key": "sk-upstream-failure-secret"
            }),
            &["模型获取失败，上游"][..],
        ),
        (
            "/api/providers/test-protocol",
            serde_json::json!({
                "provider_type": "openai_compatible",
                "base_url": "http://127.0.0.1:0",
                "api_key": "sk-upstream-failure-secret",
                "protocol": "openai",
                "model": "unavailable-model"
            }),
            &["上游测试超时", "上游连接失败", "上游测试失败"][..],
        ),
    ] {
        let (status, response): (StatusCode, serde_json::Value) =
            request_json(&app, "POST", path, Some(&credential), Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(response["ok"], false, "{path}");
        let error = response["error"].as_str().expect("operation error");
        assert!(
            expected_error_prefixes
                .iter()
                .any(|prefix| error.starts_with(prefix)),
            "{path}: {response}"
        );
    }
}
