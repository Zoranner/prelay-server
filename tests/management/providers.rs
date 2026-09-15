use axum::http::StatusCode;
use prelay_protocol::CreateProviderRequest;
use prelay_server::{app, test_support::test_state};

use crate::{auth::register, http::request_json, status::request_status};

#[tokio::test]
async fn management_provider_round_trips_model_list() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-models", "S-1-5-21-models").await;
    let credential = identity["credential"].as_str().expect("credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Selected models provider",
            "provider_type": "deepseek",
            "base_url": "https://provider-models.example",
            "api_key": "sk-models",
            "models": ["deepseek-v4-pro"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(provider["models"], serde_json::json!(["deepseek-v4-pro"]));
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}"),
        Some(credential),
        Some(serde_json::json!({ "models": ["unknown-model"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("unknown-model"),
        "{error}"
    );

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}"),
        Some(credential),
        Some(serde_json::json!({ "models": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(provider["models"], serde_json::json!([]));
}

#[tokio::test]
async fn management_credential_cannot_read_or_mutate_another_identity_provider() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity_a = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential_a = identity_a["credential"].as_str().expect("credential A");
    let request = CreateProviderRequest {
        name: "Provider A".to_string(),
        provider_type: "deepseek".to_string(),
        base_url: "https://provider-a.example".to_string(),
        api_key: "sk-a".to_string(),
        capabilities: None,
        models: None,
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
        serde_json::json!(["openai", "anthropic"])
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
async fn management_provider_response_carries_its_own_model_list() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-provider-catalog-source",
        "S-1-5-21-catalog-source",
    )
    .await;
    let credential = identity["credential"].as_str().expect("credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Catalog Provider",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-catalog-source"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        provider["models"],
        serde_json::json!(["deepseek-v4-flash", "deepseek-v4-pro"]),
        "creating without an explicit list enables every model of the catalog entry"
    );
}

#[tokio::test]
async fn management_provider_rejects_unknown_catalog_provider_type_without_updating() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-provider-validation",
        "S-1-5-21-provider-validation",
    )
    .await;
    let credential = identity["credential"].as_str().expect("credential");
    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Provider A",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-a"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}"),
        Some(credential),
        Some(serde_json::json!({ "provider_type": "not-in-catalog" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "validation_failed");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}"),
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(provider["provider_type"], "deepseek");
}

#[tokio::test]
async fn management_provider_rejects_unknown_catalog_provider_type_without_creating() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-provider-unknown-create",
        "S-1-5-21-provider-unknown-create",
    )
    .await;
    let credential = identity["credential"].as_str().expect("credential");

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Unknown Provider",
            "provider_type": "not-in-catalog",
            "base_url": "https://provider.example",
            "api_key": "sk-a",
            "models": ["deepseek-v4-flash"]
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
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-visible-to-owner-only"
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
async fn management_provider_deletion_removes_endpoint_model_references() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-provider-delete", "S-1-5-21-provider-delete").await;
    let credential = identity["credential"].as_str().expect("credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Deletable provider",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-deletable"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/endpoints",
        Some(credential),
        Some(serde_json::json!({
            "name": "Endpoint with deletable provider",
            "models": [{ "provider_id": provider_id, "upstream_model": "deepseek-v4-pro" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(endpoint["models"].as_array().map(Vec::len), Some(1));

    assert_eq!(
        request_status(
            &app,
            "DELETE",
            &format!("/api/providers/{provider_id}"),
            Some(credential),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let (status, endpoints): (StatusCode, Vec<serde_json::Value>) =
        request_json(&app, "GET", "/api/endpoints", Some(credential), None).await;
    assert_eq!(status, StatusCode::OK);
    let endpoint = endpoints.first().expect("endpoint still exists");
    assert_eq!(
        endpoint["models"].as_array().map(Vec::len),
        Some(0),
        "endpoint must not keep references to the deleted provider"
    );
}
