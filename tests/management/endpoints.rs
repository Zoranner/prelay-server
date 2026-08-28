use axum::http::StatusCode;
use prelay_protocol::CreateProviderRequest;
use prelay_server::{app, test_support::test_state};

use crate::{auth::register, http::request_json, status::request_status};

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
            "provider_type": "openai_compatible",
            "base_url": "https://provider-a.example",
            "api_key": "sk-a",
            "models": ["model-a"]
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
                "upstream_model": "model-a",
                "model_name": "public-model"
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
                    "upstream_model": "model-a",
                    "model_name": "public-model"
                },
                {
                    "provider_id": provider_id,
                    "upstream_model": " model-a ",
                    "model_name": " public-model "
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
    assert_eq!(endpoint["models"][0]["model_name"], "public-model");
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
