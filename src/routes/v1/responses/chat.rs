use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    activity::{
        enqueue_activity_content_best_effort, internal_request_text, internal_response_text,
    },
    bridge::{
        internal::InternalRequest, responses::encode::encode_responses_response,
        stream::chat_sse_response_to_responses_sse_with_stats,
    },
    error::AppError,
    models::ProviderConfig,
    observability::stream_stats::record_stream_with_activity_content,
    providers::{
        chat_completions::{decode_chat_response, encode_chat_request},
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

pub(super) async fn create_chat_response(
    state: &AppState,
    request: InternalRequest,
    provider: ProviderConfig,
    upstream_protocol: UpstreamProtocol,
    previous_response_id: Option<String>,
    context: ResponseBridgeContext,
) -> Result<Response, AppError> {
    let ResponseBridgeContext {
        identity_id,
        endpoint_name,
        model_requested,
        is_streaming,
        started_at,
    } = context;
    let request = request_with_session_history(&state.storage, &identity_id, request).await?;
    let upstream_base_url = provider_upstream_base_url(&provider, upstream_protocol);
    let upstream_url = format!(
        "{}/chat/completions",
        upstream_base_url.trim_end_matches('/')
    );
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&encode_chat_request(&request))
        .send()
        .await
        .map_err(|error| AppError::Upstream {
            status: None,
            message: format!("上游连接失败: {error}"),
        })?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        let error_message = format!("上游请求失败: {status}");
        state
            .storage
            .insert_activity(
                &identity_id,
                ActivityInsert {
                    protocol_in: "responses".to_string(),
                    protocol_out: "responses".to_string(),
                    protocol_upstream: "chat_completions".to_string(),
                    provider_id: provider.id,
                    provider_name: provider.name,
                    endpoint_name: endpoint_name.clone(),
                    model_requested,
                    model_upstream: request.model,
                    status: "failed".to_string(),
                    http_status: status.as_u16() as i64,
                    error_code: Some("upstream_status".to_string()),
                    error_message: Some(error_message.clone()),
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
                    upstream_request_id: None,
                },
            )
            .await?;
        return Err(AppError::Upstream {
            status: Some(status),
            message: error_message,
        });
    }

    if is_streaming {
        let input_text = internal_request_text(&request);
        let log = ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: endpoint_name.clone(),
            model_requested,
            model_upstream: request.model,
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
            upstream_request_id: None,
        };
        let (stream, stream_stats) =
            chat_sse_response_to_responses_sse_with_stats(upstream_response);
        let body = Body::from_stream(record_stream_with_activity_content(
            state.storage.clone(),
            identity_id.clone(),
            stream,
            log,
            started_at,
            stream_stats,
            input_text,
        ));
        return Ok((
            [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
            body,
        )
            .into_response());
    }

    let upstream_json =
        upstream_response
            .json::<Value>()
            .await
            .map_err(|_| AppError::UpstreamInvalidResponse {
                message: "上游响应格式无效".to_string(),
            })?;
    let mut response = decode_chat_response(upstream_json)?;
    response.id = format!("resp_{}", Uuid::new_v4().simple());
    let reasoning_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| usage.reasoning_tokens);
    let tool_call_count = count_tool_calls(&response);
    if request.store {
        state
            .storage
            .save_response_session(ResponseSessionInsert {
                identity_id: &identity_id,
                response_id: &response.id,
                previous_response_id: previous_response_id.as_deref(),
                provider_id: &provider.id,
                model: &response.model,
                input_messages: &request.messages,
                response: &response,
            })
            .await?;
    }
    let activity_id = state
        .storage
        .insert_activity(
            &identity_id,
            ActivityInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                endpoint_name: endpoint_name.clone(),
                model_requested,
                model_upstream: response.model.clone(),
                status: "success".to_string(),
                http_status: 200,
                error_code: None,
                error_message: None,
                is_streaming,
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
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: Some(upstream_latency_ms),
                first_token_ms: None,
                tool_call_count: Some(tool_call_count),
                upstream_request_id: None,
            },
        )
        .await?;
    enqueue_activity_content_best_effort(
        &state.storage,
        activity_id,
        &internal_request_text(&request),
        &internal_response_text(&response),
        None,
    )
    .await;

    Ok(Json(encode_responses_response(response)).into_response())
}
