use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;

use crate::{
    activity::events::{events_from_internal_request, events_from_internal_response},
    bridge::{
        internal::InternalRequest,
        stream::{native_responses_sse_with_stats, with_sse_keepalive},
    },
    error::AppError,
    image_budget::{self, ImageBudgetOutcome},
    observability::{
        stream_stats::{record_stream_with_rollout, StreamRecording, StreamRollout, StreamSession},
        upstream_observability::upstream_failure_message,
    },
    providers::spec::{provider_upstream_base_url, UpstreamProtocol},
    stats::ActivityInsert,
    storage::ResponseSessionInsert,
    AppState,
};

use super::{
    activity::{record_upstream_failure, upstream_error_details, UpstreamFailure},
    candidate::ResponseBridgeContext,
};

pub(super) async fn create_native_response(
    state: &AppState,
    payload: &Value,
    request: &InternalRequest,
    model_upstream: &str,
    provider: crate::models::ProviderConfig,
    previous_response_id: Option<String>,
    context: ResponseBridgeContext,
) -> Result<Response, AppError> {
    let upstream_base_url = provider_upstream_base_url(&provider, UpstreamProtocol::Responses);
    let upstream_url = format!("{}/responses", upstream_base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let client = state.provider_client(&provider.provider_type)?;
    let image_budget_policy = image_budget::policy();
    let budget = image_budget::apply(payload, image_budget_policy);
    let payload = match &budget {
        ImageBudgetOutcome::Disabled => payload,
        ImageBudgetOutcome::WithinBudget(report) => {
            tracing::debug!(
                total_bytes = report.total_bytes,
                max_bytes = image_budget_policy.max_bytes,
                "image payload is within the upstream budget"
            );
            payload
        }
        ImageBudgetOutcome::Trimmed { payload, report } => {
            tracing::info!(
                total_bytes = report.total_bytes,
                remaining_bytes = report.remaining_bytes,
                dropped_images = report.dropped_images,
                max_bytes = image_budget_policy.max_bytes,
                "trimmed historical images for the upstream image budget"
            );
            payload
        }
        ImageBudgetOutcome::CannotMeetBudget(report) => {
            tracing::warn!(
                total_bytes = report.total_bytes,
                remaining_bytes = report.remaining_bytes,
                dropped_images = report.dropped_images,
                max_bytes = image_budget_policy.max_bytes,
                keep_last = image_budget_policy.keep_last,
                "image payload cannot be reduced to the upstream budget"
            );
            return Err(AppError::PayloadTooLarge(format!(
                "图片请求体超过上游预算：已丢弃 {} 张历史图片，仍有 {} 字节，预算 {} 字节。请减少图片，或调整 IMAGE_BUDGET_MAX_BYTES。",
                report.dropped_images, report.remaining_bytes, image_budget_policy.max_bytes
            )));
        }
    };
    let upstream_request_body =
        crate::providers::responses::encode_native_responses_request_body(payload, model_upstream);
    let upstream_request = client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .header(header::CONTENT_TYPE, "application/json")
        .body(upstream_request_body);
    let upstream_response =
        match crate::upstream::send_with_timeout(crate::upstream::policy(), upstream_request).await
        {
            Ok(response) => response,
            Err(error) => {
                let (error_code, status, message) = upstream_error_details(&error);
                record_upstream_failure(
                    state,
                    &context,
                    &provider,
                    UpstreamFailure {
                        protocol_upstream: "responses",
                        model_upstream,
                        status,
                        error_code,
                        error_message: message,
                        upstream_latency_ms: Some(upstream_started_at.elapsed().as_millis() as i64),
                        request_events: events_from_internal_request(request),
                        conversation_id: &context.conversation_id,
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
                &context.identity_id,
                ActivityInsert {
                    protocol_in: "responses".to_string(),
                    protocol_out: "responses".to_string(),
                    protocol_upstream: "responses".to_string(),
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                    endpoint_name: context.endpoint_name.clone(),
                    model_requested: context.model_requested.clone(),
                    model_upstream: model_upstream.to_string(),
                    status: "failed".to_string(),
                    http_status: status.as_u16() as i64,
                    error_code: Some("upstream_status".to_string()),
                    error_message: Some(error_message.clone()),
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
                },
                Some(&context.conversation_id),
                events_from_internal_request(request),
                Vec::new(),
            )
            .await?;
        return Err(AppError::Upstream {
            status: Some(status),
            message: error_message,
        });
    }

    if context.is_streaming {
        let (stream, stream_stats) = match native_responses_sse_with_stats(upstream_response).await
        {
            Ok(result) => result,
            Err(error) => {
                let (error_code, status, message) = upstream_error_details(&error);
                record_upstream_failure(
                    state,
                    &context,
                    &provider,
                    UpstreamFailure {
                        protocol_upstream: "responses",
                        model_upstream,
                        status,
                        error_code,
                        error_message: message,
                        upstream_latency_ms: Some(upstream_latency_ms),
                        request_events: events_from_internal_request(request),
                        conversation_id: &context.conversation_id,
                    },
                )
                .await;
                return Err(error);
            }
        };
        let session = request.store.then(|| StreamSession {
            previous_response_id: previous_response_id.clone(),
            conversation_id: context.conversation_id.clone(),
            provider_id: provider.id.clone(),
            model: model_upstream.to_string(),
            input_messages: request.messages.clone(),
            store: true,
            response_id: None,
        });
        let log = ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "responses".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: context.endpoint_name.clone(),
            model_requested: context.model_requested,
            model_upstream: model_upstream.to_string(),
            status: "streaming".to_string(),
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
        let body = Body::from_stream(with_sse_keepalive(
            record_stream_with_rollout(
                state.activity_store.clone(),
                context.identity_id.clone(),
                stream,
                log,
                context.started_at,
                StreamRecording {
                    stats: stream_stats,
                    rollout: StreamRollout {
                        request_events: events_from_internal_request(request),
                        session_id: Some(&context.conversation_id),
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

    let response = match upstream_response.json::<Value>().await {
        Ok(response) => response,
        Err(_) => {
            let error = AppError::UpstreamInvalidResponse {
                message: "上游响应格式无效".to_string(),
            };
            let (error_code, status, message) = upstream_error_details(&error);
            record_upstream_failure(
                state,
                &context,
                &provider,
                UpstreamFailure {
                    protocol_upstream: "responses",
                    model_upstream,
                    status,
                    error_code,
                    error_message: message,
                    upstream_latency_ms: Some(upstream_latency_ms),
                    request_events: events_from_internal_request(request),
                    conversation_id: &context.conversation_id,
                },
            )
            .await;
            return Err(error);
        }
    };
    let decoded_response =
        match crate::providers::responses::decode_responses_response(response.clone()) {
            Ok(response) => response,
            Err(error) => {
                let (error_code, status, message) = upstream_error_details(&error);
                record_upstream_failure(
                    state,
                    &context,
                    &provider,
                    UpstreamFailure {
                        protocol_upstream: "responses",
                        model_upstream,
                        status,
                        error_code,
                        error_message: message,
                        upstream_latency_ms: Some(upstream_latency_ms),
                        request_events: events_from_internal_request(request),
                        conversation_id: &context.conversation_id,
                    },
                )
                .await;
                return Err(error);
            }
        };
    if request.store {
        state
            .activity_store
            .save_response_session(ResponseSessionInsert {
                identity_id: &context.identity_id,
                response_id: &decoded_response.id,
                previous_response_id: previous_response_id.as_deref(),
                conversation_id: &context.conversation_id,
                provider_id: &provider.id,
                model: &decoded_response.model,
                input_messages: &request.messages,
                response: &decoded_response,
            })
            .await?;
    }
    state
        .activity_store
        .record_completed(
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
                    .and_then(|usage| {
                        usage
                            .pointer("/input_tokens_details/cache_write_tokens")
                            .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
                            .or_else(|| usage.get("cache_creation_input_tokens"))
                    })
                    .and_then(Value::as_i64),
                latency_ms: context.started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: Some(upstream_latency_ms),
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
            Some(&context.conversation_id),
            events_from_internal_request(request),
            events_from_internal_response(&decoded_response),
        )
        .await?;

    Ok(Json(response).into_response())
}
