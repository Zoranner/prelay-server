use axum::http::StatusCode;
use prelay_protocol::CreateProviderRequest;
use prelay_server::{app, test_support::test_state};

use crate::{auth::register, http::request_json, status::request_status};

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
        models: vec!["deepseek-v4-flash".to_string()],
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
            "provider_type": "deepseek",
            "base_url": "https://provider-a.example",
            "api_key": "sk-a",
            "models": ["deepseek-v4-flash", " deepseek-v4-flash "]
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
            "api_key": "sk-a",
            "models": ["deepseek-v4-flash"]
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

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}"),
        Some(credential),
        Some(serde_json::json!({ "models": ["model-not-in-catalog"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "validation_failed");
}

#[tokio::test]
async fn management_provider_rejects_catalog_outside_model_without_creating() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-provider-model-validation",
        "S-1-5-21-provider-model-validation",
    )
    .await;
    let credential = identity["credential"].as_str().expect("credential");
    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Provider A",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-a",
            "models": ["model-not-in-catalog"]
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
async fn management_provider_rejects_empty_model_without_creating() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-provider-empty-model",
        "S-1-5-21-provider-empty-model",
    )
    .await;
    let credential = identity["credential"].as_str().expect("credential");
    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Provider A",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-a",
            "models": [" "]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "validation_failed");
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
            "api_key": "sk-visible-to-owner-only",
            "models": ["deepseek-v4-flash"]
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
async fn management_provider_model_display_name_uses_catalog_id() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-provider-display",
        "S-1-5-21-provider-display",
    )
    .await;
    let credential = identity["credential"].as_str().expect("credential");
    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Provider Display",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-display",
            "models": ["deepseek-v4-flash"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let models = provider["models"].as_array().expect("provider models");
    let known = models
        .iter()
        .find(|model| model["model_name"] == "deepseek-v4-flash")
        .expect("known model");
    assert_eq!(known["display_name"], "DeepSeek V4 Flash");
    assert_eq!(models.len(), 1);
}
