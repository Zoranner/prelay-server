use axum::http::StatusCode;
use prelay_server::{app, test_support::test_state};

use crate::{auth::register, http::request_json};

#[tokio::test]
async fn management_errors_always_carry_a_stable_code() {
    let app = app::router(test_state().await).await.expect("build app");
    let identity = register(&app, "machine-a", "S-1-5-21-100").await;
    let credential = identity["credential"].as_str().expect("credential");

    for (path, token, expected_status, expected_code) in [
        (
            "/api/providers",
            None,
            StatusCode::UNAUTHORIZED,
            "invalid_credential",
        ),
        (
            "/api/catalog/providers/unknown-provider",
            Some(credential),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
    ] {
        let (status, error): (StatusCode, serde_json::Value) =
            request_json(&app, "GET", path, token, None).await;

        assert_eq!(status, expected_status, "{path}");
        assert_eq!(error["error"]["code"], expected_code, "{path}");
        assert!(error["error"]["message"].is_string(), "{path}: {error}");
    }
}
