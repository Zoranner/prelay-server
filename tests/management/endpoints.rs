use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use prelay_server::{app, test_support::test_state};
use tower::ServiceExt;

use crate::{auth::register, http::request_json};

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
            "provider_type": "deepseek",
            "base_url": "https://provider-a.example",
            "api_key": "sk-a",
            "models": ["deepseek-v4-flash"]
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
                    "upstream_model": "deepseek-v4-flash"
                },
                {
                    "provider_id": provider_id,
                    "upstream_model": "deepseek-v4-flash"
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
async fn management_endpoint_rejects_provider_model_outside_catalog_relationship() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-endpoint-catalog",
        "S-1-5-21-endpoint-catalog",
    )
    .await;
    let credential = identity["credential"].as_str().expect("credential");
    let (status, provider): (StatusCode, serde_json::Value) = request_json(&app, "POST", "/api/providers", Some(credential), Some(serde_json::json!({"name":"DeepSeek","provider_type":"deepseek","base_url":"https://provider.example","api_key":"sk-a","models":["deepseek-v4-flash"]}))).await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, error): (StatusCode, serde_json::Value) = request_json(&app, "POST", "/api/endpoints", Some(credential), Some(serde_json::json!({"name":"Endpoint A","models":[{"provider_id":provider_id,"upstream_model":"deepseek-v4-pro"}]}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "validation_failed");
}

#[tokio::test]
async fn management_endpoint_keeps_same_model_candidates_and_distinct_ids() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-endpoint-candidates",
        "S-1-5-21-endpoint-candidates",
    )
    .await;
    let credential = identity["credential"].as_str().expect("credential");
    let mut provider_ids = Vec::new();
    for name in ["DeepSeek A", "DeepSeek B"] {
        let (status, provider): (StatusCode, serde_json::Value) = request_json(
            &app,
            "POST",
            "/api/providers",
            Some(credential),
            Some(serde_json::json!({
                "name": name,
                "provider_type": "deepseek",
                "base_url": "https://provider.example",
                "api_key": "sk-a",
                "models": ["deepseek-v4-flash", "deepseek-v4-pro"]
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        provider_ids.push(provider["id"].as_str().expect("provider id").to_string());
    }
    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/endpoints",
        Some(credential),
        Some(serde_json::json!({
            "name": "Endpoint A",
            "models": [
                { "provider_id": provider_ids[0], "upstream_model": "deepseek-v4-flash" },
                { "provider_id": provider_ids[1], "upstream_model": "deepseek-v4-flash" },
                { "provider_id": provider_ids[0], "upstream_model": "deepseek-v4-pro" }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let models = endpoint["models"].as_array().expect("endpoint models");
    assert_eq!(models.len(), 3);
    assert_eq!(models[0]["model_name"], "deepseek-v4-flash");
    assert_eq!(models[1]["model_name"], "deepseek-v4-flash");
    assert_eq!(models[2]["model_name"], "deepseek-v4-pro");
}

#[tokio::test]
async fn management_endpoint_rejects_custom_public_model_name_and_resolves_display_name() {
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
            "provider_type": "deepseek",
            "base_url": "https://provider-a.example",
            "api_key": "sk-a",
            "models": ["deepseek-v4-flash"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let custom_request = Request::builder()
        .method("POST")
        .uri("/api/endpoints")
        .header("authorization", format!("Bearer {credential}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "name": "Endpoint A",
                "models": [{
                    "provider_id": provider_id,
                    "upstream_model": "deepseek-v4-flash",
                    "modelName": "custom-public-name"
                }]
            })
            .to_string(),
        ))
        .expect("build custom model request");
    let status = app
        .clone()
        .oneshot(custom_request)
        .await
        .expect("custom model request")
        .status();
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/endpoints",
        Some(credential),
        Some(serde_json::json!({
            "name": "Endpoint A",
            "models": [{
                "provider_id": provider_id,
                "upstream_model": " deepseek-v4-flash "
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(endpoint["models"][0]["model_name"], "deepseek-v4-flash");
    assert_eq!(endpoint["models"][0]["upstream_model"], "deepseek-v4-flash");
    assert_eq!(endpoint["models"][0]["display_name"], "DeepSeek V4 Flash");
}

#[tokio::test]
async fn management_endpoint_rejects_duplicate_model_names_without_updating_the_interface() {
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
            "provider_type": "deepseek",
            "base_url": "https://provider-a.example",
            "api_key": "sk-a",
            "models": ["deepseek-v4-flash"]
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
            "name": "Endpoint A",
            "models": [{
                "provider_id": provider_id,
                "upstream_model": "deepseek-v4-flash"
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let endpoint_id = endpoint["id"].as_str().expect("endpoint id");

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/endpoints/{endpoint_id}"),
        Some(credential),
        Some(serde_json::json!({
            "name": "Changed endpoint",
            "models": [
                {
                    "provider_id": provider_id,
                    "upstream_model": "deepseek-v4-flash"
                },
                {
                    "provider_id": provider_id,
                    "upstream_model": " deepseek-v4-flash "
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"]["code"], "validation_failed");

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/endpoints/{endpoint_id}"),
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(endpoint["name"], "Endpoint A");
    assert_eq!(
        endpoint["models"]
            .as_array()
            .expect("endpoint models")
            .len(),
        1
    );
    assert_eq!(endpoint["models"][0]["model_name"], "deepseek-v4-flash");
}

#[tokio::test]
async fn management_endpoint_model_display_name_uses_catalog_id() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(
        &app,
        "machine-endpoint-display",
        "S-1-5-21-endpoint-display",
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
            "base_url": "https://provider-a.example",
            "api_key": "sk-a",
            "models": ["deepseek-v4-flash"]
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
            "name": "Endpoint Display",
            "models": [
                {
                    "provider_id": provider_id,
                    "upstream_model": "deepseek-v4-flash"
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(endpoint["models"][0]["display_name"], "DeepSeek V4 Flash");
}
