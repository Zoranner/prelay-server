use axum::http::StatusCode;
use prelay_protocol::CreateIdentityRequest;
use serde_json::Value;

use crate::{auth::valid_credential, http::request_json};

mod lifecycle;
mod quota;

async fn register_named(
    app: &axum::Router,
    machine_id: &str,
    account_sid: &str,
    display_name: &str,
) -> Value {
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
