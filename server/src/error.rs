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
                    tracing::error!(error = %message, "Internal protocol error");
                    "Internal server error".to_string()
                } else {
                    message
                };

                (status, json!({ "code": code.as_str(), "message": message }))
            }
            AppError::Internal(e) => {
                tracing::error!("Internal error: {:?}", e);
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

    use super::AppError;
    use provider_relay_protocol::ProtocolErrorCode;

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
