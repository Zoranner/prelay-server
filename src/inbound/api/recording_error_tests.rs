use axum::{body::to_bytes, http::StatusCode, response::IntoResponse};
use prelay_protocol::ProtocolErrorCode;

use crate::error::AppError;

#[tokio::test]
async fn recording_unavailable_is_a_503_without_storage_diagnostics() {
    let response =
        AppError::RecordingUnavailable("journal fsync timed out; payload=secret".to_string())
            .into_response();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read error response");
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("recording_unavailable"));
    assert!(!text.contains("secret"));
}

#[tokio::test]
async fn unsupported_extension_installation_is_a_bad_request() {
    let response = AppError::Protocol {
        code: ProtocolErrorCode::ExtensionInstallUnsupported,
        message: "扩展类型不支持安装".to_string(),
    }
    .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
