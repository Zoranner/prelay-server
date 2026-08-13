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
            AppError::Protocol { code, message } => (
                match code {
                    ProtocolErrorCode::NotFound => StatusCode::NOT_FOUND,
                    ProtocolErrorCode::InvalidCredential => StatusCode::UNAUTHORIZED,
                    ProtocolErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
                    ProtocolErrorCode::IdentityAlreadyRegistered
                    | ProtocolErrorCode::ValidationFailed => StatusCode::BAD_REQUEST,
                },
                json!({ "code": code.as_str(), "message": message }),
            ),
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
