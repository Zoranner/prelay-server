use axum::http::StatusCode;
use prelay_protocol::CreateIdentityRequest;
use prelay_server::{app, test_support::test_state};

use crate::{
    auth::{register, valid_credential},
    http::request_json,
};

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
            display_name: None,
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
