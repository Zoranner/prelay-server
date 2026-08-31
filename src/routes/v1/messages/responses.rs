use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;

use crate::{
    bridge::{
        anthropic::encode::encode_anthropic_response,
        stream::responses_sse_response_to_anthropic_messages_sse_with_stats,
    },
    error::AppError,
    observability::stream_stats::record_stream,
    providers::{
        responses::{decode_responses_response, encode_responses_request},
        spec::{provider_upstream_base_url, UpstreamProtocol},
    },
    stats::ActivityInsert,
    AppState,
};

use super::{candidate::AnthropicMessageRequestContext, count_tool_calls};

pub(super) async fn create_responses_anthropic_message(
    state: &AppState,
    request: crate::bridge::internal::InternalRequest,
    provider: crate::models::ProviderConfig,
    context: AnthropicMessageRequestContext,
) -> Result<Response, AppError> {
    let upstream_base_url = provider_upstream_base_url(&provider, UpstreamProtocol::Responses);
    let upstream_url = format!("{}/responses", upstream_base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&encode_responses_request(&request))
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
                    protocol_in: "anthropic_messages".to_string(),
                    protocol_out: "anthropic_messages".to_string(),
                    protocol_upstream: "responses".to_string(),
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
                    metadata_json: context.metadata_json.clone(),
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
            protocol_in: "anthropic_messages".to_string(),
            protocol_out: "anthropic_messages".to_string(),
            protocol_upstream: "responses".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: context.endpoint_name.clone(),
            model_requested: context.model_requested,
            model_upstream: request.model.clone(),
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
            metadata_json: context.metadata_json.clone(),
        };
        let (stream, stream_stats) = responses_sse_response_to_anthropic_messages_sse_with_stats(
            upstream_response,
            request.model,
        );
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(record_stream(
                state.storage.clone(),
                context.identity_id,
                stream,
                log,
                context.started_at,
                stream_stats,
            )))
            .map_err(|error| AppError::Internal(error.into()));
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let response = decode_responses_response(upstream_json)?;
    let reasoning_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| usage.reasoning_tokens);
    let tool_call_count = count_tool_calls(&response);
    state
        .storage
        .insert_activity(
            &context.identity_id,
            ActivityInsert {
                protocol_in: "anthropic_messages".to_string(),
                protocol_out: "anthropic_messages".to_string(),
                protocol_upstream: "responses".to_string(),
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
                reasoning_tokens,
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
                metadata_json: context.metadata_json,
            },
        )
        .await?;

    Ok(Json(encode_anthropic_response(response)).into_response())
}
