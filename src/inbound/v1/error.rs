use axum::{
    async_trait,
    extract::{rejection::JsonRejection, FromRequest, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use prelay_protocol::{ProtocolErrorCode, V1ErrorCode};
use serde_json::json;

use crate::error::AppError;
use crate::inbound::error_status::http_status_for;

/// 推理接口的错误响应。
///
/// `/v1/*` 只输出 OpenAI 错误体：
/// `{"error": {"type", "code", "param", "message"}}`。
/// `message` 只承担诊断用途；面向用户的文案由调用方按 `code` 决定。
#[derive(Debug)]
pub struct V1Error {
    status: StatusCode,
    error_type: &'static str,
    code: &'static str,
    param: Option<&'static str>,
    message: String,
}

impl V1Error {
    fn new(
        status: StatusCode,
        error_type: &'static str,
        code: V1ErrorCode,
        param: Option<&'static str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            error_type,
            code: code.as_str(),
            param,
            message: message.into(),
        }
    }

    pub fn invalid_request(param: Option<&'static str>, message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            V1ErrorCode::InvalidRequestError,
            param,
            message,
        )
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_request_error",
            V1ErrorCode::PayloadTooLarge,
            None,
            message,
        )
    }

    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            V1ErrorCode::InvalidApiKey,
            None,
            "invalid or missing API key",
        )
    }

    pub async fn route_not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            V1ErrorCode::NotFound,
            None,
            "route does not exist",
        )
    }

    pub async fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "invalid_request_error",
            V1ErrorCode::MethodNotAllowed,
            None,
            "method is not allowed for this route",
        )
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            V1ErrorCode::Internal,
            None,
            "Internal server error",
        )
    }

    fn upstream(status: StatusCode, code: V1ErrorCode, message: impl Into<String>) -> Self {
        let error_type = if status == StatusCode::TOO_MANY_REQUESTS {
            "rate_limit_error"
        } else {
            "api_error"
        };
        Self::new(status, error_type, code, None, message)
    }

    fn from_protocol(code: ProtocolErrorCode, message: String) -> Self {
        if code == ProtocolErrorCode::Internal {
            return Self::internal();
        }
        let status = http_status_for(code);
        let v1_code = match status.as_u16() {
            401 => V1ErrorCode::InvalidApiKey,
            403 => V1ErrorCode::PermissionDenied,
            404 => V1ErrorCode::NotFound,
            503 => V1ErrorCode::ServiceUnavailable,
            400 | 422 => V1ErrorCode::InvalidRequestError,
            _ => V1ErrorCode::Internal,
        };
        let error_type = match status.as_u16() {
            401 => "authentication_error",
            500 | 503 => "server_error",
            _ => "invalid_request_error",
        };
        let message = if status.is_server_error() && status != StatusCode::SERVICE_UNAVAILABLE {
            "internal error".to_string()
        } else {
            message
        };
        Self::new(status, error_type, v1_code, None, message)
    }
}

impl From<AppError> for V1Error {
    fn from(error: AppError) -> Self {
        match error {
            AppError::OpenAi {
                status,
                error_type,
                code,
                param,
                message,
            } => Self {
                status,
                error_type,
                code,
                param,
                message,
            },
            AppError::Unauthorized => Self::unauthorized(),
            AppError::NotFound(message) => Self::new(
                StatusCode::NOT_FOUND,
                "invalid_request_error",
                V1ErrorCode::NotFound,
                None,
                message,
            ),
            AppError::BadRequest(message) => Self::invalid_request(None, message),
            AppError::PayloadTooLarge(message) => Self::payload_too_large(message),
            AppError::Upstream { status, message } => Self::upstream(
                status.unwrap_or(StatusCode::BAD_GATEWAY),
                V1ErrorCode::UpstreamStatus,
                message,
            ),
            AppError::UpstreamInvalidResponse { message } => Self::upstream(
                StatusCode::BAD_GATEWAY,
                V1ErrorCode::UpstreamInvalidResponse,
                message,
            ),
            AppError::Protocol { code, message } => Self::from_protocol(code, message),
            AppError::RecordingUnavailable(_) => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_error",
                V1ErrorCode::RecordingUnavailable,
                None,
                "recording service is unavailable",
            ),
            AppError::Internal(_) => Self::internal(),
        }
    }
}

impl IntoResponse for V1Error {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "type": self.error_type,
                "code": self.code,
                "param": self.param,
                "message": self.message,
            }
        });
        (self.status, Json(body)).into_response()
    }
}

/// 推理接口的 JSON 提取器。
///
/// 请求体解析失败、Content-Type 不对或超出体积上限时同样输出 OpenAI 错误体，
/// 不落到 axum 的纯文本拒绝响应。
pub struct V1Json<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for V1Json<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = V1Error;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(V1Error::from_json_rejection)?;
        Ok(Self(value))
    }
}

impl V1Error {
    fn from_json_rejection(rejection: JsonRejection) -> Self {
        let status = rejection.status();
        let message = rejection.body_text();
        if status.is_client_error() {
            return Self::new(
                status,
                "invalid_request_error",
                V1ErrorCode::InvalidRequestError,
                None,
                message,
            );
        }
        tracing::error!(
            error_code = V1ErrorCode::Internal.as_str(),
            http_status = status.as_u16(),
            "request body rejection on the inference surface"
        );
        Self::internal()
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::{V1Error, V1Json};

    async fn body(response: axum::response::Response) -> Value {
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("read response");
        serde_json::from_slice(&bytes).expect("json error body")
    }

    #[tokio::test]
    async fn unknown_routes_and_method_mismatches_use_the_openai_envelope() {
        let app = Router::new()
            .route("/known", get(|| async { "ok" }))
            .fallback(V1Error::route_not_found)
            .method_not_allowed_fallback(V1Error::method_not_allowed);

        let missing = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/missing")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("route request");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let payload = body(missing).await;
        assert_eq!(payload["error"]["code"], "not_found");
        assert_eq!(payload["error"]["type"], "invalid_request_error");

        let mismatch = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/known")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("route request");
        assert_eq!(mismatch.status(), StatusCode::METHOD_NOT_ALLOWED);
        let payload = body(mismatch).await;
        assert_eq!(payload["error"]["code"], "method_not_allowed");
    }

    #[tokio::test]
    async fn request_body_rejections_use_the_openai_envelope() {
        let app = Router::new().route(
            "/payload",
            post(|V1Json(_): V1Json<Value>| async { StatusCode::NO_CONTENT }),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/payload")
                    .header("content-type", "application/json")
                    .body(Body::from("{ not json"))
                    .expect("build request"),
            )
            .await
            .expect("route request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let payload = body(response).await;
        assert_eq!(payload["error"]["code"], "invalid_request_error");
        assert!(payload["error"]["message"].is_string());
    }

    #[tokio::test]
    async fn nested_routes_keep_the_openai_fallback() {
        let inner = Router::new()
            .route("/known", get(|| async { "ok" }))
            .fallback(V1Error::route_not_found);
        let app = Router::new().nest("/v1", inner);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/missing")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("route request");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let payload = body(response).await;
        assert_eq!(payload["error"]["code"], "not_found");
    }
}
