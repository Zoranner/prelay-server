use axum::http::StatusCode;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use prelay_protocol::CreateIdentityRequest;

use crate::http::request_json;

pub async fn register(
    app: &axum::Router,
    machine_id: &str,
    account_sid: &str,
) -> serde_json::Value {
    let credential = valid_credential(&format!("{machine_id}-{account_sid}"));
    let request = CreateIdentityRequest {
        machine_id: machine_id.to_string(),
        account_sid: account_sid.to_string(),
        credential: credential.clone(),
        display_name: None,
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

pub fn valid_credential(seed: &str) -> String {
    let mut bytes = [0_u8; 32];
    for (index, byte) in seed.bytes().take(bytes.len()).enumerate() {
        bytes[index] = byte;
    }
    URL_SAFE_NO_PAD.encode(bytes)
}
