use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;

use crate::{
    activity::{insert_activity_with_content, internal_request_text, internal_response_text},
    bridge::{internal::InternalRequest, stream::native_responses_sse_with_stats},
    error::AppError,
    observability::stream_stats::record_stream,
    providers::spec::{provider_upstream_base_url, UpstreamProtocol},
    stats::ActivityInsert,
    storage::ResponseSessionInsert,
    AppState,
};

use super::candidate::ResponseBridgeContext;

pub(super) async fn create_native_response(
    state: &AppState,
    payload: Value,
    provider: crate::models::ProviderConfig,
    request: InternalRequest,
    previous_response_id: Option<String>,
    context: ResponseBridgeContext,
) -> Result<Response, AppError> {
    let upstream_base_url = provider_upstream_base_url(&provider, UpstreamProtocol::Responses);
    let upstream_url = format!("{}/responses", upstream_base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
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
                    protocol_in: "responses".to_string(),
                    protocol_out: "responses".to_string(),
                    protocol_upstream: "responses".to_string(),
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
        let log = ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "responses".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: context.endpoint_name.clone(),
            model_requested: context.model_requested,
            model_upstream: payload
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
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
        let (stream, stream_stats) = native_responses_sse_with_stats(upstream_response);
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

    let response = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let decoded_response =
        crate::providers::responses::decode_responses_response(response.clone())?;
    state
        .storage
        .save_response_session(ResponseSessionInsert {
            identity_id: &context.identity_id,
            response_id: &decoded_response.id,
            previous_response_id: previous_response_id.as_deref(),
            provider_id: &provider.id,
            model: &decoded_response.model,
            input_messages: &request.messages,
            response: &decoded_response,
        })
        .await?;
    insert_activity_with_content(
        &state.storage,
        &context.identity_id,
        ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "responses".to_string(),
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
                .and_then(|usage| {
                    usage
                        .pointer("/input_tokens_details/cached_tokens")
                        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
                        .or_else(|| usage.get("cache_read_input_tokens"))
                })
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
        &internal_request_text(&request),
        &internal_response_text(&decoded_response),
        None,
    )
    .await?;

    Ok(Json(response).into_response())
}
