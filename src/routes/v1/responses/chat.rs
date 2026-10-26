use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    activity::events::{events_from_internal_request, events_from_internal_response},
    bridge::{
        internal::InternalRequest,
        responses::decode::validate_internal_request_for_bridge,
        responses::encode::encode_responses_response,
        stream::{chat_sse_response_to_responses_sse_with_stats, with_sse_keepalive},
    },
    error::AppError,
    models::ProviderConfig,
    observability::{
        stream_stats::{record_stream_with_rollout, StreamRecording, StreamRollout, StreamSession},
        upstream_observability::upstream_failure_message,
    },
    providers::{
        chat_completions::{decode_chat_response, encode_chat_request},
        spec::{provider_upstream_base_url, UpstreamProtocol},
    },
    stats::ActivityInsert,
    storage::ResponseSessionInsert,
    AppState,
};

use super::{
    activity::{record_upstream_failure, upstream_error_details, UpstreamFailure},
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
        conversation_id,
        started_at,
    } = context;
    let failure_context = ResponseBridgeContext {
        identity_id: identity_id.clone(),
        endpoint_name: endpoint_name.clone(),
        model_requested: model_requested.clone(),
        is_streaming,
        conversation_id: conversation_id.clone(),
        started_at,
    };
    let request = request_with_session_history(&state.storage, &identity_id, request).await?;
    validate_internal_request_for_bridge(&request)?;
    let upstream_base_url = provider_upstream_base_url(&provider, upstream_protocol);
    let upstream_url = format!(
        "{}/chat/completions",
        upstream_base_url.trim_end_matches('/')
    );
    let upstream_started_at = std::time::Instant::now();
    let client = state.provider_client(&provider.provider_type)?;
    let request_builder = client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&encode_chat_request(&request));
    let upstream_response = match crate::upstream::send_with_timeout(
        crate::upstream::policy(),
        request_builder,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let (error_code, status, message) = upstream_error_details(&error);
            record_upstream_failure(
                state,
                &failure_context,
                &provider,
                UpstreamFailure {
                    protocol_upstream: "chat_completions",
                    model_upstream: &request.model,
                    status,
                    error_code,
                    error_message: message,
                    upstream_latency_ms: Some(upstream_started_at.elapsed().as_millis() as i64),
                    request_events: events_from_internal_request(&request),
                    conversation_id: &failure_context.conversation_id,
                },
            )
            .await;
            return Err(error);
        }
    };
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        let upstream_body = upstream_response.text().await.unwrap_or_default();
        let error_message = upstream_failure_message(status, &upstream_body);
        state
            .activity_store
            .record_completed(
                &identity_id,
                ActivityInsert {
                    protocol_in: "responses".to_string(),
                    protocol_out: "responses".to_string(),
                    protocol_upstream: "chat_completions".to_string(),
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                    provider_type: provider.provider_type.clone(),
                    endpoint_name: endpoint_name.clone(),
                    model_requested: model_requested.clone(),
                    model_upstream: request.model.clone(),
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
                previous_response_id.as_deref(),
                events_from_internal_request(&request),
                Vec::new(),
            )
            .await?;
        return Err(AppError::Upstream {
            status: Some(status),
            message: error_message,
        });
    }

    if is_streaming {
        let response_id = format!("resp_{}", Uuid::new_v4().simple());
        let (stream, stream_stats) = match chat_sse_response_to_responses_sse_with_stats(
            upstream_response,
            response_id.clone(),
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                let (error_code, status, message) = upstream_error_details(&error);
                record_upstream_failure(
                    state,
                    &failure_context,
                    &provider,
                    UpstreamFailure {
                        protocol_upstream: "chat_completions",
                        model_upstream: &request.model,
                        status,
                        error_code,
                        error_message: message,
                        upstream_latency_ms: Some(upstream_latency_ms),
                        request_events: events_from_internal_request(&request),
                        conversation_id: &failure_context.conversation_id,
                    },
                )
                .await;
                return Err(error);
            }
        };
        let session = request.store.then(|| StreamSession {
            previous_response_id: previous_response_id.clone(),
            conversation_id: conversation_id.clone(),
            provider_id: provider.id.clone(),
            model: request.model.clone(),
            input_messages: request.messages.clone(),
            store: true,
            response_id: Some(response_id),
        });
        let log = ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            provider_type: provider.provider_type,
            endpoint_name: endpoint_name.clone(),
            model_requested,
            model_upstream: request.model.clone(),
            status: "streaming".to_string(),
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
        let body = Body::from_stream(with_sse_keepalive(
            record_stream_with_rollout(
                state.activity_store.clone(),
                identity_id.clone(),
                stream,
                log,
                started_at,
                StreamRecording {
                    stats: stream_stats,
                    rollout: StreamRollout {
                        request_events: events_from_internal_request(&request),
                        session_id: Some(&conversation_id),
                    },
                    session,
                },
            )
            .await?,
        ));
        return Ok((
            [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
            body,
        )
            .into_response());
    }

    let upstream_json = match upstream_response.json::<Value>().await {
        Ok(response) => response,
        Err(_) => {
            let error = AppError::UpstreamInvalidResponse {
                message: "上游响应格式无效".to_string(),
            };
            let (error_code, status, message) = upstream_error_details(&error);
            record_upstream_failure(
                state,
                &failure_context,
                &provider,
                UpstreamFailure {
                    protocol_upstream: "chat_completions",
                    model_upstream: &request.model,
                    status,
                    error_code,
                    error_message: message,
                    upstream_latency_ms: Some(upstream_latency_ms),
                    request_events: events_from_internal_request(&request),
                    conversation_id: &failure_context.conversation_id,
                },
            )
            .await;
            return Err(error);
        }
    };
    let mut response = match decode_chat_response(upstream_json) {
        Ok(response) => response,
        Err(error) => {
            let (error_code, status, message) = upstream_error_details(&error);
            record_upstream_failure(
                state,
                &failure_context,
                &provider,
                UpstreamFailure {
                    protocol_upstream: "chat_completions",
                    model_upstream: &request.model,
                    status,
                    error_code,
                    error_message: message,
                    upstream_latency_ms: Some(upstream_latency_ms),
                    request_events: events_from_internal_request(&request),
                    conversation_id: &failure_context.conversation_id,
                },
            )
            .await;
            return Err(error);
        }
    };
    response.id = format!("resp_{}", Uuid::new_v4().simple());
    let reasoning_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| usage.reasoning_tokens);
    let tool_call_count = count_tool_calls(&response);
    if request.store {
        state
            .activity_store
            .save_response_session(ResponseSessionInsert {
                identity_id: &identity_id,
                response_id: &response.id,
                previous_response_id: previous_response_id.as_deref(),
                conversation_id: &conversation_id,
                provider_id: &provider.id,
                model: &response.model,
                input_messages: &request.messages,
                response: &response,
            })
            .await?;
    }
    state
        .activity_store
        .record_completed(
            &identity_id,
            ActivityInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                provider_type: provider.provider_type,
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
            Some(&conversation_id),
            events_from_internal_request(&request),
            events_from_internal_response(&response),
        )
        .await?;

    Ok(Json(encode_responses_response(response)).into_response())
}
