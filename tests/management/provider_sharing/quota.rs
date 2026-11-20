use axum::http::StatusCode;
use prelay_server::{bootstrap as app, test_support::test_state};

use crate::http::request_json;

use super::register_named;

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
            "visibility": "selected",
            "users": [{
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
    assert_eq!(sharing["visibility"], "selected");
    assert_eq!(sharing["users"][0]["quota"][0]["window"], "day");
    assert_eq!(sharing["users"][0]["quota"][0]["limit"], 1000);

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
            "visibility": "selected",
            "users": [{
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
            "visibility": "selected",
            "users": [
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
            "visibility": "selected",
            "users": [{
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
