use axum::http::StatusCode;
use prelay_protocol::CreateIdentityRequest;
use prelay_server::{bootstrap as app, test_support::test_state};

use crate::{
    auth::{register, valid_credential},
    http::request_json,
    status::request_status,
};

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
async fn provider_visibility_controls_lists_and_replaces_selected_grantees() {
    let app = app::router(test_state().await).await.expect("build app");
    let owner = register_named(&app, "sharing-owner", "S-1-5-21-owner", "Owner").await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let grantee = register_named(&app, "sharing-grantee", "S-1-5-21-grantee", "Grantee").await;
    let grantee_credential = grantee["credential"].as_str().expect("grantee credential");
    let replacement = register_named(
        &app,
        "sharing-replacement",
        "S-1-5-21-replacement",
        "Replacement",
    )
    .await;
    let replacement_credential = replacement["credential"]
        .as_str()
        .expect("replacement credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Shared Provider",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-owner-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, providers): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/providers",
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());

    let (status, sharing): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(owner_credential),
        Some(serde_json::json!({
            "shares": [{
                "identity_id": grantee["identity_id"],
                "quota": null
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sharing["shares"][0]["identity_id"], grantee["identity_id"]);
    assert!(sharing["shares"][0]["quota"].is_null());

    let (status, providers): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/providers",
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0]["owner_identity_id"], owner["identity_id"]);
    assert_eq!(providers[0]["owner_display_name"], "Owner");
    assert_eq!(providers[0]["can_manage"], false);
    assert_eq!(
        providers[0]["selected_identity_ids"],
        serde_json::json!([grantee["identity_id"]])
    );
    assert!(providers[0].get("api_key").is_none());
    assert!(providers[0].get("api_key_masked").is_none());

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}"),
        Some(grantee_credential),
        Some(serde_json::json!({ "name": "Should be rejected" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "provider_sharing_not_allowed");

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(grantee_credential),
        Some(serde_json::json!({
            "shares": []
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(owner_credential),
        Some(serde_json::json!({
            "shares": [{
                "identity_id": replacement["identity_id"],
                "quota": null
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, providers): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/providers",
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());

    let (status, providers): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/providers",
        Some(replacement_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers.len(), 1);
    assert_eq!(
        providers[0]["selected_identity_ids"],
        serde_json::json!([replacement["identity_id"]])
    );

    let (status, sharing): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(owner_credential),
        Some(serde_json::json!({
            "shares": []
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(sharing["shares"].as_array().expect("shares").is_empty());

    let (status, providers): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/providers",
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(providers.is_empty());
}

#[tokio::test]
async fn shared_provider_can_back_endpoint_until_revoked_and_deleted() {
    let app = app::router(test_state().await).await.expect("build app");
    let owner = register(&app, "endpoint-sharing-owner", "S-1-5-21-owner").await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let grantee = register(&app, "endpoint-sharing-grantee", "S-1-5-21-grantee").await;
    let grantee_credential = grantee["credential"].as_str().expect("grantee credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Endpoint Shared Provider",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-shared-secret"
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
            "shares": [{
                "identity_id": grantee["identity_id"],
                "quota": null
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/endpoints",
        Some(grantee_credential),
        Some(serde_json::json!({
            "name": "Shared Endpoint",
            "models": [{
                "provider_id": provider_id,
                "upstream_model": "deepseek-v4-pro"
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let endpoint_id = endpoint["id"].as_str().expect("endpoint id");
    let endpoint_token = endpoint["token"].as_str().expect("endpoint token");

    let (status, models): (StatusCode, serde_json::Value) =
        request_json(&app, "GET", "/v1/models", Some(endpoint_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models["data"][0]["id"], "deepseek-v4-pro");

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(owner_credential),
        Some(serde_json::json!({
            "shares": []
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/endpoints/{endpoint_id}"),
        Some(grantee_credential),
        Some(serde_json::json!({
            "models": [{
                "provider_id": provider_id,
                "upstream_model": "deepseek-v4-pro"
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["code"], "provider_not_usable");

    let (status, models): (StatusCode, serde_json::Value) =
        request_json(&app, "GET", "/v1/models", Some(endpoint_token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(models["data"].as_array().expect("models data").is_empty());

    assert_eq!(
        request_status(
            &app,
            "DELETE",
            &format!("/api/providers/{provider_id}"),
            Some(owner_credential),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        &format!("/api/endpoints/{endpoint_id}"),
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(endpoint["models"]
        .as_array()
        .expect("endpoint models")
        .is_empty());
}

#[tokio::test]
async fn non_owner_can_ping_a_shared_provider_but_cannot_test_its_protocols() {
    let app = app::router(test_state().await).await.expect("build app");
    let owner = register(&app, "operation-sharing-owner", "S-1-5-21-owner").await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let grantee = register(&app, "operation-sharing-grantee", "S-1-5-21-grantee").await;
    let grantee_credential = grantee["credential"].as_str().expect("grantee credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Operation Shared Provider",
            "provider_type": "deepseek",
            "base_url": "https://provider.example",
            "api_key": "sk-operation-secret"
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
            "shares": [{
                "identity_id": grantee["identity_id"],
                "quota": null
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, ping): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/providers/{provider_id}/ping"),
        Some(grantee_credential),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ping["ok"], false);
    assert!(!ping.to_string().contains("sk-operation-secret"));

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        &format!("/api/providers/{provider_id}/test-protocol"),
        Some(grantee_credential),
        Some(serde_json::json!({
            "protocol": "openai",
            "model": "deepseek-v4-pro"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "provider_sharing_not_allowed");
}

#[tokio::test]
async fn limited_share_reports_the_shared_quota_windows() {
    let app = app::router(test_state().await).await.expect("build app");
    let owner = register_named(&app, "quota-owner", "S-1-5-21-quota-owner", "Quota Owner").await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let grantee = register_named(
        &app,
        "quota-grantee",
        "S-1-5-21-quota-grantee",
        "Quota Grantee",
    )
    .await;
    let grantee_credential = grantee["credential"].as_str().expect("grantee credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Quota Provider",
            "provider_type": "gotoken",
            "base_url": "https://gotoken.cc",
            "api_key": "sk-quota-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, sharing): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(owner_credential),
        Some(serde_json::json!({
            "shares": [{
                "identity_id": grantee["identity_id"],
                "quota": [{
                    "window": "day",
                    "limit": 1000,
                    "accumulate": true,
                    "reset_time": "00:00",
                    "reset_weekday": null,
                    "reset_day": null
                }]
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sharing["shares"][0]["quota"][0]["window"], "day");
    assert_eq!(sharing["shares"][0]["quota"][0]["limit"], 1000);

    let (status, quotas): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/providers/quotas",
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(quotas.len(), 1);
    assert_eq!(quotas[0]["mode"], "shared_limited");
    assert_eq!(quotas[0]["quota"]["windows"][0]["label"], "1 天");
    assert_eq!(quotas[0]["quota"]["windows"][0]["used_percent"], 0.0);
    assert_eq!(quotas[0]["quota"]["windows"][0]["limit"], "1000");
    assert_eq!(quotas[0]["quota"]["windows"][0]["amount"], "1000");

    let (status, owner_quotas): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/providers/quotas",
        Some(owner_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_quotas[0]["quota"]["error"]["code"], "unsupported");
}

#[tokio::test]
async fn exhausted_shared_quota_is_rejected_before_the_upstream_call() {
    let app = app::router(test_state().await).await.expect("build app");
    let owner = register_named(
        &app,
        "quota-limit-owner",
        "S-1-5-21-quota-limit-owner",
        "Quota Limit Owner",
    )
    .await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let grantee = register_named(
        &app,
        "quota-limit-grantee",
        "S-1-5-21-quota-limit-grantee",
        "Quota Limit Grantee",
    )
    .await;
    let grantee_credential = grantee["credential"].as_str().expect("grantee credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Limited Provider",
            "provider_type": "gotoken",
            "base_url": "https://gotoken.cc",
            "api_key": "sk-limited-secret",
            "models": ["gpt-6-astra"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "provider response: {provider}");
    let provider_id = provider["id"].as_str().expect("provider id");

    let (status, _): (StatusCode, serde_json::Value) = request_json(
        &app,
        "PATCH",
        &format!("/api/providers/{provider_id}/sharing"),
        Some(owner_credential),
        Some(serde_json::json!({
            "shares": [{
                "identity_id": grantee["identity_id"],
                "quota": [{
                    "window": "day",
                    "limit": 1,
                    "accumulate": false,
                    "reset_time": "00:00",
                    "reset_weekday": null,
                    "reset_day": null
                }]
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, endpoint): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/endpoints",
        Some(grantee_credential),
        Some(serde_json::json!({
            "name": "Limited endpoint",
            "models": [{
                "provider_id": provider_id,
                "upstream_model": "gpt-6-astra"
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let endpoint_token = endpoint["token"].as_str().expect("endpoint token");

    let (status, error): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/v1/chat/completions",
        Some(endpoint_token),
        Some(serde_json::json!({
            "model": "gpt-6-astra",
            "messages": [{ "role": "user", "content": "hello" }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let message = error["error"]
        .as_str()
        .or_else(|| error["error"]["message"].as_str())
        .unwrap_or_default();
    assert!(
        message.contains("共享额度不足"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn each_grantee_gets_a_separate_shared_quota() {
    let app = app::router(test_state().await).await.expect("build app");
    let owner = register_named(
        &app,
        "quota-split-owner",
        "S-1-5-21-quota-split-owner",
        "Quota Split Owner",
    )
    .await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let first = register_named(
        &app,
        "quota-split-first",
        "S-1-5-21-quota-split-first",
        "Quota Split First",
    )
    .await;
    let first_credential = first["credential"].as_str().expect("first credential");
    let second = register_named(
        &app,
        "quota-split-second",
        "S-1-5-21-quota-split-second",
        "Quota Split Second",
    )
    .await;
    let second_credential = second["credential"].as_str().expect("second credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Split Quota Provider",
            "provider_type": "gotoken",
            "base_url": "https://gotoken.cc",
            "api_key": "sk-split-secret"
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
            "shares": [
                {
                    "identity_id": first["identity_id"],
                    "quota": [{
                        "window": "day",
                        "limit": 1000,
                        "accumulate": false,
                        "reset_time": "00:00",
                        "reset_weekday": null,
                        "reset_day": null
                    }]
                },
                {
                    "identity_id": second["identity_id"],
                    "quota": [{
                        "window": "day",
                        "limit": 2000,
                        "accumulate": false,
                        "reset_time": "00:00",
                        "reset_weekday": null,
                        "reset_day": null
                    }]
                }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for (credential, expected_limit) in [(first_credential, "1000"), (second_credential, "2000")] {
        let (status, quotas): (StatusCode, Vec<serde_json::Value>) =
            request_json(&app, "GET", "/api/providers/quotas", Some(credential), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(quotas.len(), 1);
        assert_eq!(quotas[0]["mode"], "shared_limited");
        assert_eq!(quotas[0]["quota"]["windows"][0]["limit"], expected_limit);
        assert_eq!(quotas[0]["quota"]["windows"][0]["amount"], expected_limit);
    }
}

#[tokio::test]
async fn direct_share_uses_the_owner_quota_mode() {
    let app = app::router(test_state().await).await.expect("build app");
    let owner = register_named(
        &app,
        "quota-direct-owner",
        "S-1-5-21-quota-direct-owner",
        "Quota Direct Owner",
    )
    .await;
    let owner_credential = owner["credential"].as_str().expect("owner credential");
    let grantee = register_named(
        &app,
        "quota-direct-grantee",
        "S-1-5-21-quota-direct-grantee",
        "Quota Direct Grantee",
    )
    .await;
    let grantee_credential = grantee["credential"].as_str().expect("grantee credential");

    let (status, provider): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(owner_credential),
        Some(serde_json::json!({
            "name": "Direct Quota Provider",
            "provider_type": "gotoken",
            "base_url": "https://gotoken.cc",
            "api_key": "sk-direct-secret"
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
            "shares": [{
                "identity_id": grantee["identity_id"],
                "quota": null
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, quotas): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/providers/quotas",
        Some(grantee_credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(quotas.len(), 1);
    assert_eq!(quotas[0]["mode"], "shared_unlimited");
    assert_eq!(quotas[0]["quota"]["error"]["code"], "unsupported");
}
