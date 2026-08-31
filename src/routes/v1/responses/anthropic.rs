use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    activity::{insert_activity_with_content, internal_request_text, internal_response_text},
    bridge::{
        internal::InternalRequest, responses::encode::encode_responses_response,
        stream::anthropic_messages_sse_response_to_responses_sse_with_stats,
    },
    error::AppError,
    observability::stream_stats::record_stream,
    providers::{
        anthropic_messages::{
            decode_anthropic_messages_response, encode_anthropic_messages_request,
        },
        spec::{provider_upstream_base_url, UpstreamProtocol},
    },
    stats::ActivityInsert,
    storage::ResponseSessionInsert,
    AppState,
};

use super::{
    candidate::ResponseBridgeContext,
    sessions::{count_tool_calls, request_with_session_history},
};

pub(super) async fn create_anthropic_messages_response(
    state: &AppState,
    mut request: InternalRequest,
    provider: crate::models::ProviderConfig,
    previous_response_id: Option<String>,
    context: ResponseBridgeContext,
) -> Result<Response, AppError> {
    request = request_with_session_history(&state.storage, &context.identity_id, request).await?;
    let upstream_base_url =
        provider_upstream_base_url(&provider, UpstreamProtocol::AnthropicMessages);
    let upstream_url = format!("{}/messages", upstream_base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .header("x-api-key", &provider.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&encode_anthropic_messages_request(&request))
        .send()
        .await
        .map_err(|error| AppError::Upstream {
            status: None,
            message: format!("上游连接失败: {error}"),
        })?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        state
            .storage
            .insert_activity(
                &context.identity_id,
                ActivityInsert {
                    protocol_in: "responses".to_string(),
                    protocol_out: "responses".to_string(),
                    protocol_upstream: "anthropic_messages".to_string(),
                    provider_id: provider.id,
                    provider_name: provider.name,
                    endpoint_name: context.endpoint_name.clone(),
                    model_requested: context.model_requested,
                    model_upstream: request.model,
                    status: "failed".to_string(),
                    http_status: status.as_u16() as i64,
                    error_code: None,
                    error_message: None,
                    is_streaming: context.is_streaming,
                    input_tokens: None,
                    output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    latency_ms: context.started_at.elapsed().as_millis() as i64,
                    upstream_latency_ms: None,
                    first_token_ms: None,
                    tool_call_count: None,
                    upstream_request_id: None,
                },
            )
            .await?;
        return Err(AppError::Upstream {
            status: Some(status),
            message: format!("上游请求失败: {status}"),
        });
    }

    if context.is_streaming {
        let log = ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "anthropic_messages".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: context.endpoint_name.clone(),
            model_requested: context.model_requested,
            model_upstream: request.model,
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming: context.is_streaming,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            latency_ms: context.started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
        };
        let (stream, stream_stats) =
            anthropic_messages_sse_response_to_responses_sse_with_stats(upstream_response);
        let body = Body::from_stream(record_stream(
            state.storage.clone(),
            context.identity_id.clone(),
            stream,
            log,
            context.started_at,
            stream_stats,
        ));
        return Ok((
            [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
            body,
        )
            .into_response());
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let mut response = decode_anthropic_messages_response(upstream_json)?;
    response.id = format!("resp_{}", Uuid::new_v4().simple());
    let tool_call_count = count_tool_calls(&response);
    state
        .storage
        .save_response_session(ResponseSessionInsert {
            identity_id: &context.identity_id,
            response_id: &response.id,
            previous_response_id: previous_response_id.as_deref(),
            provider_id: &provider.id,
            model: &response.model,
            input_messages: &request.messages,
            response: &response,
        })
        .await?;
    insert_activity_with_content(
        &state.storage,
        &context.identity_id,
        ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "anthropic_messages".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: context.endpoint_name.clone(),
            model_requested: context.model_requested,
            model_upstream: response.model.clone(),
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming: context.is_streaming,
            input_tokens: response.usage.as_ref().and_then(|usage| usage.input_tokens),
            output_tokens: response
                .usage
                .as_ref()
                .and_then(|usage| usage.output_tokens),
            reasoning_tokens: None,
            cache_read_tokens: response
                .usage
                .as_ref()
                .and_then(|usage| usage.cache_read_tokens),
            cache_write_tokens: response
                .usage
                .as_ref()
                .and_then(|usage| usage.cache_write_tokens),
            latency_ms: context.started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: Some(tool_call_count),
            upstream_request_id: None,
        },
        &internal_request_text(&request),
        &internal_response_text(&response),
        None,
    )
    .await?;

    Ok(Json(encode_responses_response(response)).into_response())
}
