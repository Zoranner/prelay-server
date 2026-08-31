use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use futures::TryStreamExt;
use serde_json::Value;

use crate::{
    error::AppError,
    observability::stream_stats::record_first_chunk,
    providers::spec::{provider_upstream_base_url, UpstreamProtocol},
    stats::ActivityInsert,
    AppState,
};

use super::candidate::AnthropicMessageRequestContext;

pub(super) async fn create_native_anthropic_message(
    state: &AppState,
    payload: Value,
    provider: crate::models::ProviderConfig,
    context: AnthropicMessageRequestContext,
) -> Result<Response, AppError> {
    let upstream_base_url =
        provider_upstream_base_url(&provider, UpstreamProtocol::AnthropicMessages);
    let upstream_url = format!("{}/messages", upstream_base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .header("x-api-key", &provider.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&payload)
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
                    protocol_upstream: "anthropic_messages".to_string(),
                    provider_id: provider.id,
                    provider_name: provider.name,
                    endpoint_name: context.endpoint_name.clone(),
                    model_requested: context.model_requested,
                    model_upstream: "unknown".to_string(),
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
        let model_upstream = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let log = ActivityInsert {
            protocol_in: "anthropic_messages".to_string(),
            protocol_out: "anthropic_messages".to_string(),
            protocol_upstream: "anthropic_messages".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: context.endpoint_name.clone(),
            model_requested: context.model_requested,
            model_upstream,
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

        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(record_first_chunk(
                state.storage.clone(),
                context.identity_id,
                upstream_response
                    .bytes_stream()
                    .map_err(std::io::Error::other),
                log,
                context.started_at,
            )))
            .map_err(|error| AppError::Internal(error.into()));
    }

    let response = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    state
        .storage
        .insert_activity(
            &context.identity_id,
            ActivityInsert {
                protocol_in: "anthropic_messages".to_string(),
                protocol_out: "anthropic_messages".to_string(),
                protocol_upstream: "anthropic_messages".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                endpoint_name: context.endpoint_name.clone(),
                model_requested: context.model_requested,
                model_upstream: response
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                status: "success".to_string(),
                http_status: 200,
                error_code: None,
                error_message: None,
                is_streaming: context.is_streaming,
                input_tokens: response
                    .get("usage")
                    .and_then(|usage| usage.get("input_tokens"))
                    .and_then(Value::as_i64),
                output_tokens: response
                    .get("usage")
                    .and_then(|usage| usage.get("output_tokens"))
                    .and_then(Value::as_i64),
                reasoning_tokens: None,
                cache_read_tokens: response
                    .get("usage")
                    .and_then(|usage| usage.get("cache_read_input_tokens"))
                    .and_then(Value::as_i64),
                cache_write_tokens: response
                    .get("usage")
                    .and_then(|usage| usage.get("cache_creation_input_tokens"))
                    .and_then(Value::as_i64),
                latency_ms: context.started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: Some(upstream_latency_ms),
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
                metadata_json: context.metadata_json,
            },
        )
        .await?;

    Ok(Json(response).into_response())
}
