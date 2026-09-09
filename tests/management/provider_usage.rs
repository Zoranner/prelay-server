use axum::http::StatusCode;
use prelay_protocol::{CreateIdentityRequest, ProviderUsageResponse};
use prelay_server::{stats::ActivityInsert, storage::Storage};

use crate::{
    auth::{register, valid_credential},
    http::request_json,
    test_context::test_context,
};

async fn seed_provider_activity(
    storage: &Storage,
    identity_id: &str,
    activity_id: &str,
    provider_id: &str,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    status: &str,
) {
    storage
        .insert_activity_with_id(
            identity_id,
            activity_id.to_string(),
            ActivityInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "responses".to_string(),
                endpoint_name: "Shared endpoint".to_string(),
                provider_id: provider_id.to_string(),
                provider_name: "Shared Provider".to_string(),
                model_requested: "shared-model".to_string(),
                model_upstream: "shared-model".to_string(),
                status: status.to_string(),
                http_status: 200,
                input_tokens,
                output_tokens,
                ..Default::default()
            },
        )
        .await
        .expect("seed provider activity");
}

async fn register_named(
    app: &axum::Router,
    machine_id: &str,
    account_sid: &str,
    display_name: &str,
) -> serde_json::Value {
    let credential = valid_credential(&format!("{machine_id}-{account_sid}"));
    let (status, mut response): (StatusCode, serde_json::Value) = request_json(
        app,
        "POST",
        "/api/identities",
        None,
        Some(
            serde_json::to_value(CreateIdentityRequest {
                machine_id: machine_id.to_string(),
                account_sid: account_sid.to_string(),
                credential: credential.clone(),
                display_name: Some(display_name.to_string()),
            })
            .expect("serialize identity request"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    response["credential"] = credential.into();
    response
}

#[tokio::test]
async fn visible_users_read_complete_provider_usage_without_sensitive_data() {
    let context = test_context().await;
    let app = context.app;
    let owner = register_named(&app, "usage-owner", "S-1-5-21-usage-owner", "Usage Owner").await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let owner_id = owner["identity_id"].as_str().expect("owner identity id");
    let grantee = register_named(
        &app,
        "usage-grantee",
        "S-1-5-21-usage-grantee",
        "Usage Grantee",
    )
    .await;
    let grantee_credential = grantee["credential"].as_str().expect("grantee credential");
    let grantee_id = grantee["identity_id"]
        .as_str()
        .expect("grantee identity id");
    let outsider = register_named(
        &app,
        "usage-outsider",
        "S-1-5-21-usage-outsider",
        "Usage Outsider",
    )
    .await;
    let outsider_credential = outsider["credential"]
        .as_str()
        .expect("outsider credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Shared Provider",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-provider-usage-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(owner_credential),
        Some(serde_json::json!({
            "visibility": "selected",
            "identity_ids": [grantee_id]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    seed_provider_activity(
        &context.storage,
        owner_id,
        "usage-owner-success",
        provider_id,
        Some(3),
        Some(4),
        "success",
    )
    .await;
    seed_provider_activity(
        &context.storage,
        grantee_id,
        "usage-grantee-failed",
        provider_id,
        None,
        Some(6),
        "failed",
    )
    .await;
    seed_provider_activity(
        &context.storage,
        grantee_id,
        "usage-grantee-success",
        provider_id,
        Some(5),
        None,
        "success",
    )
    .await;

    let (status, owner_usage): (StatusCode, ProviderUsageResponse) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/usage?range=today"),
        Some(owner_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_usage.total_requests, 3);
    assert_eq!(owner_usage.input_tokens, 8);
    assert_eq!(owner_usage.output_tokens, 10);
    assert_eq!(owner_usage.total_tokens, 18);
    assert!(owner_usage.latest_used_at.is_some());
    assert_eq!(owner_usage.users.len(), 2);
    assert_eq!(owner_usage.users[0].identity_id, grantee_id);
    assert_eq!(owner_usage.users[0].display_name, "Usage Grantee");
    assert_eq!(owner_usage.users[0].request_count, 2);
    assert_eq!(owner_usage.users[0].input_tokens, 5);
    assert_eq!(owner_usage.users[0].output_tokens, 6);
    assert_eq!(owner_usage.users[0].total_tokens, 11);
    assert_eq!(owner_usage.users[1].identity_id, owner_id);
    assert_eq!(owner_usage.users[1].display_name, "Usage Owner");
    assert_eq!(owner_usage.users[1].request_count, 1);
    assert_eq!(owner_usage.users[1].total_tokens, 7);

    let (status, grantee_usage): (StatusCode, ProviderUsageResponse) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/usage?range=today"),
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(grantee_usage, owner_usage);

    let (status, yesterday_usage): (StatusCode, ProviderUsageResponse) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/usage?range=yesterday"),
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(yesterday_usage.total_requests, 0);
    assert_eq!(yesterday_usage.input_tokens, 0);
    assert_eq!(yesterday_usage.output_tokens, 0);
    assert_eq!(yesterday_usage.total_tokens, 0);
    assert!(yesterday_usage.latest_used_at.is_none());
    assert!(yesterday_usage.users.is_empty());

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/usage?range=today"),
        Some(outsider_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"]["code"], "provider_not_visible");

    let serialized = serde_json::to_string(&owner_usage).expect("serialize usage response");
    for sensitive in [
        "sk-provider-usage-secret",
        "device_credential",
        "endpoint_token",
        "upstream_request",
        "response_content",
    ] {
        assert!(!serialized.contains(sensitive), "usage leaked {sensitive}");
    }
}

#[tokio::test]
async fn visible_provider_without_usage_returns_zero_usage() {
    let context = test_context().await;
    let app = context.app;
    let owner = register(&app, "usage-empty-owner", "S-1-5-21-empty-owner").await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Empty Provider",
            "provider_type": "deepseek",
            "base_url": "https://empty-provider.example",
            "api_key": "sk-empty-provider-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, usage): (StatusCode, ProviderUsageResponse) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/usage"),
        Some(owner_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        usage,
        ProviderUsageResponse {
            total_requests: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            latest_used_at: None,
            users: Vec::new(),
        }
    );
}

#[tokio::test]
async fn every_registered_identity_can_read_complete_usage_for_an_all_shared_provider() {
    let context = test_context().await;
    let app = context.app;
    let owner = register_named(
        &app,
        "usage-all-owner",
        "S-1-5-21-usage-all-owner",
        "All Usage Owner",
    )
    .await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let owner_id = owner["identity_id"].as_str().expect("owner identity id");
    let reader = register_named(
        &app,
        "usage-all-reader",
        "S-1-5-21-usage-all-reader",
        "All Usage Reader",
    )
    .await;
    let reader_credential = reader["credential"].as_str().expect("reader credential");
    let reader_id = reader["identity_id"].as_str().expect("reader identity id");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "All Shared Provider",
            "provider_type": "deepseek",
            "base_url": "https://all-shared-provider.example",
            "api_key": "sk-all-shared-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(owner_credential),
        Some(serde_json::json!({
            "visibility": "all",
            "identity_ids": []
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    seed_provider_activity(
        &context.storage,
        owner_id,
        "usage-all-owner-request",
        provider_id,
        Some(10),
        Some(2),
        "success",
    )
    .await;
    seed_provider_activity(
        &context.storage,
        reader_id,
        "usage-all-reader-request",
        provider_id,
        Some(4),
        Some(6),
        "failed",
    )
    .await;

    let (status, owner_usage): (StatusCode, ProviderUsageResponse) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/usage?range=all"),
        Some(owner_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_usage.total_requests, 2);
    assert_eq!(owner_usage.input_tokens, 14);
    assert_eq!(owner_usage.output_tokens, 8);
    assert_eq!(owner_usage.total_tokens, 22);
    assert_eq!(owner_usage.users.len(), 2);
    let owner_user = owner_usage
        .users
        .iter()
        .find(|user| user.identity_id == owner_id)
        .expect("owner usage");
    assert_eq!(owner_user.display_name, "All Usage Owner");
    assert_eq!(owner_user.request_count, 1);
    assert_eq!(owner_user.total_tokens, 12);
    let reader_user = owner_usage
        .users
        .iter()
        .find(|user| user.identity_id == reader_id)
        .expect("reader usage");
    assert_eq!(reader_user.display_name, "All Usage Reader");
    assert_eq!(reader_user.request_count, 1);
    assert_eq!(reader_user.total_tokens, 10);

    let (status, reader_usage): (StatusCode, ProviderUsageResponse) = request_json(
        &app,
        "GET",
        &format!("/api/providers/{provider_id}/usage?range=all"),
        Some(reader_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reader_usage, owner_usage);
}
