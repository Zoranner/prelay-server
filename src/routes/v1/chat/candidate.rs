use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use futures::TryStreamExt;
use serde_json::Value;

use crate::{
    activity::{chat_message_text, chat_response_text, insert_activity_with_content},
    error::AppError,
    observability::{
        stream_stats::record_first_chunk, upstream_observability::upstream_observability,
    },
    providers::spec::provider_upstream_base_url,
    routes::v1::{auth::CurrentProtocolAccess, endpoint_resolver::ResolvedEndpointProvider},
    stats::ActivityInsert,
    AppState,
};

pub(super) async fn create_chat_completion_with_candidate(
    state: &AppState,
    access: &CurrentProtocolAccess,
    mut payload: Value,
    model: String,
    is_streaming: bool,
    started_at: std::time::Instant,
    resolved: ResolvedEndpointProvider,
) -> Result<Response, AppError> {
    let provider = resolved.provider;
    let model_upstream = resolved.model_upstream;

    payload["model"] = Value::String(model_upstream.clone());
    let upstream_base_url = provider_upstream_base_url(&provider, resolved.upstream_protocol);
    let upstream_url = format!(
        "{}/chat/completions",
        upstream_base_url.trim_end_matches('/')
    );
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
        let observability_headers = upstream_response.headers().clone();
        let error_body = upstream_response.text().await.ok();
        let observability = upstream_observability(&observability_headers, error_body.as_deref());
        state
            .storage
            .insert_activity(
                &access.identity_id,
                ActivityInsert {
                    protocol_in: "chat_completions".to_string(),
                    protocol_out: "chat_completions".to_string(),
                    protocol_upstream: "chat_completions".to_string(),
                    provider_id: provider.id,
                    provider_name: provider.name,
                    endpoint_name: access.endpoint_name.clone(),
                    model_requested: model.clone(),
                    model_upstream,
                    status: "failed".to_string(),
                    http_status: status.as_u16() as i64,
                    error_code: None,
                    error_message: observability.error_message,
                    is_streaming,
                    input_tokens: None,
                    output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    latency_ms: started_at.elapsed().as_millis() as i64,
                    upstream_latency_ms: None,
                    first_token_ms: None,
                    tool_call_count: None,
                    upstream_request_id: observability.request_id,
                },
            )
            .await?;
        return Err(AppError::Upstream {
            status: Some(status),
            message: format!("上游请求失败: {status}"),
        });
    }

    if is_streaming {
        let upstream_request_id =
            upstream_observability(upstream_response.headers(), None).request_id;
        let log = ActivityInsert {
            protocol_in: "chat_completions".to_string(),
            protocol_out: "chat_completions".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: access.endpoint_name.clone(),
            model_requested: model,
            model_upstream,
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id,
        };
        let body = Body::from_stream(record_first_chunk(
            state.storage.clone(),
            access.identity_id.clone(),
            upstream_response
                .bytes_stream()
                .map_err(std::io::Error::other),
            log,
            started_at,
        ));
        return Ok(([(header::CONTENT_TYPE, "text/event-stream")], body).into_response());
    }

    let upstream_request_id = upstream_observability(upstream_response.headers(), None).request_id;
    let response = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    insert_activity_with_content(
        &state.storage,
        &access.identity_id,
        ActivityInsert {
            protocol_in: "chat_completions".to_string(),
            protocol_out: "chat_completions".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: access.endpoint_name.clone(),
            model_requested: model,
            model_upstream: response
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming,
            input_tokens: response
                .get("usage")
                .and_then(|usage| usage.get("prompt_tokens"))
                .and_then(Value::as_i64),
            output_tokens: response
                .get("usage")
                .and_then(|usage| usage.get("completion_tokens"))
                .and_then(Value::as_i64),
            reasoning_tokens: response
                .get("usage")
                .and_then(|usage| usage.get("completion_tokens_details"))
                .and_then(|details| details.get("reasoning_tokens"))
                .and_then(Value::as_i64),
            cache_read_tokens: response
                .get("usage")
                .and_then(|usage| {
                    usage
                        .pointer("/prompt_tokens_details/cached_tokens")
                        .or_else(|| usage.get("cache_read_input_tokens"))
                })
                .and_then(Value::as_i64),
            cache_write_tokens: response
                .get("usage")
                .and_then(|usage| usage.get("cache_creation_input_tokens"))
                .and_then(Value::as_i64),
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id,
        },
        &chat_message_text(&payload),
        &chat_response_text(&response),
        None,
    )
    .await?;

    Ok(Json(response).into_response())
}
