use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use provider_relay_protocol::ProtocolErrorCode;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    Unauthorized,
    BadRequest(String),
    Protocol {
        code: ProtocolErrorCode,
        message: String,
    },
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, json!(message)),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, json!("Invalid or missing token")),
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, json!(message)),
            AppError::Protocol { code, message } => {
                let status = match code {
                    ProtocolErrorCode::NotFound => StatusCode::NOT_FOUND,
                    ProtocolErrorCode::InvalidCredential => StatusCode::UNAUTHORIZED,
                    ProtocolErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
                    ProtocolErrorCode::IdentityAlreadyRegistered
                    | ProtocolErrorCode::ValidationFailed => StatusCode::BAD_REQUEST,
                };
                let message = if code == ProtocolErrorCode::Internal {
                    tracing::error!(
                        error_code = code.as_str(),
                        http_status = status.as_u16(),
                        "Internal protocol error"
                    );
                    "Internal server error".to_string()
                } else {
                    message
                };

                (status, json!({ "code": code.as_str(), "message": message }))
            }
            AppError::Internal(_error) => {
                tracing::error!(
                    error_code = ProtocolErrorCode::Internal.as_str(),
                    http_status = StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    "Internal server error"
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!("Internal server error"),
                )
            }
        };

        (status, Json(json!({ "error": error }))).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Internal(e.into())
    }
}

impl From<crate::storage::StorageError> for AppError {
    fn from(error: crate::storage::StorageError) -> Self {
        Self::Protocol {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

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
    use provider_relay_protocol::ProtocolErrorCode;

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
            message: "SQLite error: no such table: provider_keys; credential=secret-value"
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
                    "code": "internal",
                    "message": "Internal server error"
                }
            })
        );
        assert!(!String::from_utf8_lossy(&body).contains("provider_keys"));
        assert!(!String::from_utf8_lossy(&body).contains("secret-value"));
    }
}
