use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use prelay_protocol::{ProtocolErrorCode, V1ErrorCode};

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    Unauthorized,
    BadRequest(String),
    PayloadTooLarge(String),
    Upstream {
        status: Option<StatusCode>,
        message: String,
    },
    UpstreamInvalidResponse {
        message: String,
    },
    Protocol {
        code: ProtocolErrorCode,
        message: String,
    },
    OpenAi {
        status: StatusCode,
        error_type: &'static str,
        code: &'static str,
        param: Option<&'static str>,
        message: String,
    },
    RecordingUnavailable(String),
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match &self {
            AppError::Protocol {
                code: ProtocolErrorCode::Internal,
                ..
            } => {
                tracing::error!(
                    error_code = ProtocolErrorCode::Internal.as_str(),
                    http_status = StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    "Internal protocol error"
                );
            }
            AppError::Internal(_) => {
                tracing::error!(
                    error_code = ProtocolErrorCode::Internal.as_str(),
                    http_status = StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    "Internal server error"
                );
            }
            AppError::RecordingUnavailable(_) => {
                tracing::error!(
                    error_code = V1ErrorCode::RecordingUnavailable.as_str(),
                    http_status = StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                    "Recording service is unavailable"
                );
            }
            _ => {}
        }

        crate::inbound::v1::error::V1Error::from(self).into_response()
    }
}

impl AppError {
    pub(crate) fn openai_invalid(param: &'static str, message: impl Into<String>) -> Self {
        Self::OpenAi {
            status: StatusCode::BAD_REQUEST,
            error_type: "invalid_request_error",
            code: V1ErrorCode::InvalidRequestError.as_str(),
            param: Some(param),
            message: message.into(),
        }
    }

    pub(crate) fn openai_unsupported(param: &'static str, message: impl Into<String>) -> Self {
        Self::OpenAi {
            status: StatusCode::BAD_REQUEST,
            error_type: "invalid_request_error",
            code: V1ErrorCode::UnsupportedParameter.as_str(),
            param: Some(param),
            message: message.into(),
        }
    }

    pub(crate) fn openai_previous_response_not_found() -> Self {
        Self::OpenAi {
            status: StatusCode::BAD_REQUEST,
            error_type: "invalid_request_error",
            code: V1ErrorCode::PreviousResponseNotFound.as_str(),
            param: Some("previous_response_id"),
            message: "Previous response not found".to_string(),
        }
    }

    pub fn is_retryable_upstream(&self) -> bool {
        match self {
            Self::Upstream { status: None, .. } => true,
            Self::Upstream {
                status: Some(status),
                ..
            } => matches!(status.as_u16(), 408 | 429 | 500..=599),
            _ => false,
        }
    }

    pub fn is_failoverable_upstream(&self) -> bool {
        matches!(
            self,
            Self::Upstream { .. } | Self::UpstreamInvalidResponse { .. }
        )
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e)
    }
}

#[cfg(test)]
#[path = "inbound/api/recording_error_tests.rs"]
mod extra_tests;

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode, response::IntoResponse};
    use serde_json::json;
    use std::{
        fmt::{self, Write as _},
        sync::{Arc, Mutex},
    };
    use tracing::{
        field::{Field, Visit},
        Event, Subscriber,
    };
    use tracing_subscriber::{layer::Context, prelude::*, Layer, Registry};

    use super::AppError;
    use crate::store::storage::StorageError;
    use prelay_protocol::ProtocolErrorCode;

    #[derive(Clone)]
    struct EventCapture {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl<S> Layer<S> for EventCapture
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut visitor = FieldVisitor(String::new());
            event.record(&mut visitor);
            self.events
                .lock()
                .expect("lock captured tracing events")
                .push(format!("{} {}", event.metadata().name(), visitor.0));
        }
    }

    struct FieldVisitor(String);

    impl Visit for FieldVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            let _ = write!(self.0, "{}={value:?};", field.name());
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            let _ = write!(self.0, "{}={value};", field.name());
        }
    }

    #[test]
    fn internal_protocol_error_tracing_event_excludes_sensitive_details() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = Registry::default().with(EventCapture {
            events: Arc::clone(&events),
        });

        tracing::subscriber::with_default(subscriber, || {
            let _ = AppError::Protocol {
                code: ProtocolErrorCode::Internal,
                message: "provider_key=provider-secret; credential=secret-value".to_string(),
            }
            .into_response();
        });

        let events = events.lock().expect("lock captured tracing events");
        assert_eq!(events.len(), 1);
        assert!(!events[0].contains("provider_key"));
        assert!(!events[0].contains("secret-value"));
        assert!(events[0].contains("error_code=internal"));
    }

    #[test]
    fn internal_error_tracing_event_excludes_sensitive_details() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = Registry::default().with(EventCapture {
            events: Arc::clone(&events),
        });

        tracing::subscriber::with_default(subscriber, || {
            let _ = AppError::Internal(anyhow::anyhow!(
                "device_credential=device-secret; provider_key=provider-secret"
            ))
            .into_response();
        });

        let events = events.lock().expect("lock captured tracing events");
        assert_eq!(events.len(), 1);
        assert!(!events[0].contains("device_credential"));
        assert!(!events[0].contains("provider_key"));
        assert!(!events[0].contains("device-secret"));
        assert!(events[0].contains("error_code=internal"));
    }

    #[tokio::test]
    async fn internal_protocol_error_hides_storage_diagnostics() {
        let response = AppError::Protocol {
            code: ProtocolErrorCode::Internal,
            message: "PostgreSQL error: no such table: provider_keys; credential=secret-value"
                .to_string(),
        }
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read error response");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("error response is JSON");

        assert_eq!(
            payload,
            json!({
                "error": {
                    "type": "server_error",
                    "code": "internal",
                    "param": null,
                    "message": "Internal server error"
                }
            })
        );
        assert!(!String::from_utf8_lossy(&body).contains("provider_keys"));
        assert!(!String::from_utf8_lossy(&body).contains("secret-value"));
    }

    #[tokio::test]
    async fn crypto_storage_error_hides_ciphertext_diagnostics() {
        let response = AppError::from(StorageError::Crypto(
            "ciphertext=base64-payload; nonce=nonce-value; secret=provider-key".to_string(),
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read error response");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("error response is JSON");

        assert_eq!(
            payload,
            json!({
                "error": {
                    "type": "server_error",
                    "code": "internal",
                    "param": null,
                    "message": "Internal server error"
                }
            })
        );
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("ciphertext"));
        assert!(!body.contains("nonce-value"));
        assert!(!body.contains("provider-key"));
    }

    #[tokio::test]
    async fn preserves_a_safe_upstream_http_status_for_clients() {
        let response = AppError::Upstream {
            status: Some(StatusCode::FORBIDDEN),
            message: "upstream request failed: status=403 Forbidden".to_string(),
        }
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read error response");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("error response is JSON"),
            json!({
                "error": {
                    "type": "api_error",
                    "code": "upstream_status",
                    "param": null,
                    "message": "upstream request failed: status=403 Forbidden"
                }
            })
        );
    }

    #[tokio::test]
    async fn payload_too_large_is_a_413() {
        let response =
            AppError::PayloadTooLarge("image payload exceeds the upstream budget".to_string())
                .into_response();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read error response");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("error response is JSON"),
            json!({
                "error": {
                    "type": "invalid_request_error",
                    "code": "payload_too_large",
                    "param": null,
                    "message": "image payload exceeds the upstream budget"
                }
            })
        );
    }

    #[tokio::test]
    async fn every_inference_error_uses_the_openai_envelope() {
        for (error, expected_status, expected_code) in [
            (
                AppError::Unauthorized,
                StatusCode::UNAUTHORIZED,
                "invalid_api_key",
            ),
            (
                AppError::NotFound("route does not exist".to_string()),
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                AppError::BadRequest("model must not be empty".to_string()),
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
            ),
            (
                AppError::Protocol {
                    code: ProtocolErrorCode::ProviderNotVisible,
                    message: "provider is not visible to identity".to_string(),
                },
                StatusCode::NOT_FOUND,
                "not_found",
            ),
        ] {
            let response = error.into_response();
            assert_eq!(response.status(), expected_status);

            let body = to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("read error response");
            let payload: serde_json::Value =
                serde_json::from_slice(&body).expect("error response is JSON");

            assert_eq!(payload["error"]["code"], expected_code);
            assert!(payload["error"]["type"].is_string());
            assert!(payload["error"]["message"].is_string());
            assert!(payload["error"].get("param").is_some());
        }
    }

    #[tokio::test]
    async fn invalid_extension_content_is_unprocessable() {
        let response = AppError::Protocol {
            code: ProtocolErrorCode::ExtensionContentInvalid,
            message: "扩展内容无效".to_string(),
        }
        .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn provider_protocol_errors_use_semantic_http_statuses() {
        for (code, expected_status) in [
            (
                ProtocolErrorCode::InvalidProviderSharing,
                StatusCode::BAD_REQUEST,
            ),
            (
                ProtocolErrorCode::ProviderSharingNotAllowed,
                StatusCode::FORBIDDEN,
            ),
            (ProtocolErrorCode::ProviderNotVisible, StatusCode::NOT_FOUND),
            (
                ProtocolErrorCode::ProviderNotUsable,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                ProtocolErrorCode::ProviderRouteUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let response = AppError::Protocol {
                code,
                message: "provider error".to_string(),
            }
            .into_response();

            assert_eq!(response.status(), expected_status, "{}", code.as_str());
        }
    }

    #[tokio::test]
    async fn openai_error_uses_the_protocol_envelope() {
        let response = AppError::OpenAi {
            status: StatusCode::BAD_REQUEST,
            error_type: "invalid_request_error",
            code: "unsupported_parameter",
            param: Some("conversation"),
            message: "conversation is not supported yet".to_string(),
        }
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read error response");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("JSON error"),
            json!({
                "error": {
                    "type": "invalid_request_error",
                    "code": "unsupported_parameter",
                    "param": "conversation",
                    "message": "conversation is not supported yet"
                }
            })
        );
    }
}
