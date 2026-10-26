use axum::http::StatusCode;

use crate::{
    activity::RolloutEventBody, error::AppError, models::ProviderConfig, stats::ActivityInsert,
    AppState,
};

use super::candidate::ResponseBridgeContext;

pub(super) fn upstream_error_details(error: &AppError) -> (&'static str, StatusCode, String) {
    match error {
        AppError::Upstream {
            status: Some(StatusCode::GATEWAY_TIMEOUT),
            message,
        } => (
            "upstream_response_head_timeout",
            StatusCode::GATEWAY_TIMEOUT,
            message.clone(),
        ),
        AppError::Upstream {
            status: Some(status),
            message,
        } => ("upstream_status", *status, message.clone()),
        AppError::Upstream {
            status: None,
            message,
        } => ("upstream_connect", StatusCode::BAD_GATEWAY, message.clone()),
        AppError::UpstreamInvalidResponse { message } => (
            "upstream_invalid_response",
            StatusCode::BAD_GATEWAY,
            message.clone(),
        ),
        _ => (
            "upstream_error",
            StatusCode::BAD_GATEWAY,
            "上游请求失败".to_string(),
        ),
    }
}

pub(super) struct UpstreamFailure<'a> {
    pub protocol_upstream: &'a str,
    pub model_upstream: &'a str,
    pub status: StatusCode,
    pub error_code: &'a str,
    pub error_message: String,
    pub upstream_latency_ms: Option<i64>,
    pub request_events: Vec<RolloutEventBody>,
    pub conversation_id: &'a str,
}

pub(super) async fn record_upstream_failure(
    state: &AppState,
    context: &ResponseBridgeContext,
    provider: &ProviderConfig,
    failure: UpstreamFailure<'_>,
) {
    let UpstreamFailure {
        protocol_upstream,
        model_upstream,
        status,
        error_code,
        error_message,
        upstream_latency_ms,
        request_events,
        conversation_id,
    } = failure;
    let latency_ms = context.started_at.elapsed().as_millis() as i64;
    let activity = ActivityInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: protocol_upstream.to_string(),
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        provider_type: provider.provider_type.clone(),
        endpoint_name: context.endpoint_name.clone(),
        model_requested: context.model_requested.clone(),
        model_upstream: model_upstream.to_string(),
        status: "failed".to_string(),
        http_status: status.as_u16() as i64,
        error_code: Some(error_code.to_string()),
        error_message: Some(error_message),
        is_streaming: context.is_streaming,
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms,
        upstream_latency_ms,
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: None,
    };
    if let Err(error) = state
        .activity_store
        .record_completed(
            &context.identity_id,
            activity,
            Some(conversation_id),
            request_events,
            Vec::new(),
        )
        .await
    {
        tracing::error!(
            error_code = error.code().as_str(),
            "failed to record upstream failure activity"
        );
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::upstream_error_details;
    use crate::error::AppError;

    #[test]
    fn maps_response_head_timeout_to_a_stable_code() {
        let error = AppError::Upstream {
            status: Some(StatusCode::GATEWAY_TIMEOUT),
            message: "上游在 240 秒内未返回响应头".to_string(),
        };

        assert_eq!(
            upstream_error_details(&error),
            (
                "upstream_response_head_timeout",
                StatusCode::GATEWAY_TIMEOUT,
                "上游在 240 秒内未返回响应头".to_string(),
            )
        );
    }

    #[test]
    fn maps_invalid_upstream_response_to_bad_gateway() {
        let error = AppError::UpstreamInvalidResponse {
            message: "上游响应格式无效".to_string(),
        };

        assert_eq!(
            upstream_error_details(&error),
            (
                "upstream_invalid_response",
                StatusCode::BAD_GATEWAY,
                "上游响应格式无效".to_string(),
            )
        );
    }
}
