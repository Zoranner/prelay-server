use axum::http::StatusCode;

use prelay_server::{app, test_support::test_state};

use crate::{auth::register, http::request_json};

#[tokio::test]
async fn model_removed_from_provider_list_is_neither_listed_nor_routable() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-provider-models", "S-1-5-21-provider-models").await;
    let credential = identity["credential"].as_str().expect("credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential),
        Some(serde_json::json!({
            "name": "Provider with full model list",
            "provider_type": "deepseek",
            "base_url": "http://127.0.0.1:1",
            "api_key": "sk-models"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        provider["models"],
        serde_json::json!(["deepseek-v4-flash", "deepseek-v4-pro"])
    );
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/endpoints",
        Some(credential),
        Some(serde_json::json!({
            "name": "Provider models endpoint",
            "models": [{ "provider_id": provider_id, "upstream_model": "deepseek-v4-pro" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = endpoint["token"].as_str().expect("endpoint token");

    let (status, models): (StatusCode, serde_json::Value) =
        request_json(&app, "GET", "/v1/models", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(model_ids(&models).contains(&"deepseek-v4-pro".to_string()));

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}"),
        Some(credential),
        Some(serde_json::json!({ "models": ["deepseek-v4-flash"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, models): (StatusCode, serde_json::Value) =
        request_json(&app, "GET", "/v1/models", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!model_ids(&models).contains(&"deepseek-v4-pro".to_string()));

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/v1/chat/completions",
        Some(token),
        Some(serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [{ "role": "user", "content": "hello" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]
            .as_str()
            .unwrap_or_default()
            .contains("不在供应商的模型清单里"),
        "{error}"
    );
}

fn model_ids(models: &serde_json::Value) -> Vec<String> {
    models["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
