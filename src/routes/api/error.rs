use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use prelay_protocol::{ProtocolErrorBody, ProtocolErrorCode, ProtocolErrorResponse};

use crate::error::{http_status_for, AppError};

/// 管理 API 的错误响应。
///
/// 管理面与 `/v1` 协议面共用 [`AppError`] 作为内部错误，但对外只输出稳定错误码：
/// `{"error": {"code": ..., "message": ...}}`。`message` 只承担诊断用途，
/// 面向用户的文案由客户端按 `code` 决定。
///
/// 管理面自己产生的错误必须用 [`ApiError`] 的构造函数显式给出 `code`；
/// 只有存储层、共享工具函数返回的 [`AppError`] 才经 `From` 适配，
/// 此时按错误类别落到对应 code。
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: ProtocolErrorCode,
    message: String,
}

impl ApiError {
    pub fn new(code: ProtocolErrorCode, message: impl Into<String>) -> Self {
        Self {
            status: http_status_for(code),
            code,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ProtocolErrorCode::NotFound, message)
    }

    pub fn validation_failed(message: impl Into<String>) -> Self {
        Self::new(ProtocolErrorCode::ValidationFailed, message)
    }

    pub fn invalid_credential() -> Self {
        Self::new(
            ProtocolErrorCode::InvalidCredential,
            "invalid or missing device credential",
        )
    }

    pub fn internal(error: impl Into<anyhow::Error>) -> Self {
        // 与其他内部错误一致：只记录事件，不把诊断内容写进日志或响应。
        let _error = error.into();
        Self::internal_response()
    }

    fn internal_response() -> Self {
        internal_error_log(StatusCode::INTERNAL_SERVER_ERROR);
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: ProtocolErrorCode::Internal,
            message: "Internal server error".to_string(),
        }
    }

    fn with_status(mut self, status: Option<StatusCode>) -> Self {
        if let Some(status) = status {
            self.status = status;
        }
        self
    }
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Protocol {
                code: ProtocolErrorCode::Internal,
                ..
            } => Self::internal_response(),
            AppError::Protocol { code, message } => Self::new(code, message),
            AppError::NotFound(message) => Self::not_found(message),
            AppError::Unauthorized => Self::invalid_credential(),
            AppError::BadRequest(message) => Self::validation_failed(message),
            // 管理面不承载上游故障：上游结果通过 ProviderOperationResponse 返回。
            // 这里保留上游状态和消息，避免把诊断信息替换成通用的内部错误。
            AppError::Upstream { status, message } => {
                Self::new(ProtocolErrorCode::Internal, message).with_status(status)
            }
            AppError::UpstreamInvalidResponse { message } => {
                Self::new(ProtocolErrorCode::Internal, message)
                    .with_status(Some(StatusCode::BAD_GATEWAY))
            }
            AppError::Internal(error) => Self::internal(error),
        }
    }
}

impl From<crate::storage::StorageError> for ApiError {
    fn from(error: crate::storage::StorageError) -> Self {
        Self::from(AppError::from(error))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ProtocolErrorBody::new(self.code, self.message);
        (self.status, Json(ProtocolErrorResponse { error: body })).into_response()
    }
}

fn internal_error_log(status: StatusCode) {
    tracing::error!(
        error_code = ProtocolErrorCode::Internal.as_str(),
        http_status = status.as_u16(),
        "Internal server error"
    );
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use serde_json::Value;

    use super::*;

    async fn error_payload(error: AppError) -> (StatusCode, Value) {
        let response = ApiError::from(error).into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("read error response");
        (
            status,
            serde_json::from_slice(&body).expect("error response is JSON"),
        )
    }

    #[tokio::test]
    async fn internal_error_variants_are_reported_with_their_code() {
        for (error, expected_status, expected_code) in [
            (
                AppError::NotFound("catalog entry does not exist".to_string()),
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                AppError::Unauthorized,
                StatusCode::UNAUTHORIZED,
                "invalid_credential",
            ),
            (
                AppError::BadRequest("protocol is not supported".to_string()),
                StatusCode::BAD_REQUEST,
                "validation_failed",
            ),
            (
                AppError::Protocol {
                    code: ProtocolErrorCode::ProviderNotVisible,
                    message: "provider is not visible to identity".to_string(),
                },
                StatusCode::NOT_FOUND,
                "provider_not_visible",
            ),
        ] {
            let (status, payload) = error_payload(error).await;
            assert_eq!(status, expected_status);
            assert_eq!(payload["error"]["code"], expected_code);
            assert!(payload["error"]["message"].is_string());
        }
    }

    #[tokio::test]
    async fn internal_errors_hide_diagnostics_behind_a_stable_code() {
        let (status, payload) = error_payload(AppError::Internal(anyhow::anyhow!(
            "provider_key=provider-secret; credential=secret-value"
        )))
        .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(payload["error"]["code"], "internal");
        assert_eq!(payload["error"]["message"], "Internal server error");
        assert!(!payload.to_string().contains("provider-secret"));
        assert!(!payload.to_string().contains("secret-value"));
    }

    #[tokio::test]
    async fn upstream_failures_keep_their_status_and_diagnostic_message() {
        let (status, payload) = error_payload(AppError::Upstream {
            status: Some(StatusCode::TOO_MANY_REQUESTS),
            message: "upstream rejected the request".to_string(),
        })
        .await;

        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(payload["error"]["code"], "internal");
        assert_eq!(payload["error"]["message"], "upstream rejected the request");
    }
}
