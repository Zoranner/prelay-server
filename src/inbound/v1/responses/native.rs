use std::borrow::Cow;

use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{
    codec::activity::{events_from_internal_request, events_from_internal_response},
    codec::providers::spec::{
        provider_upstream_base_url, with_provider_headers, RequestHeaderContext, UpstreamProtocol,
    },
    codec::{
        bridge::stream::{native_responses_sse_with_stats, with_sse_keepalive},
        internal::InternalRequest,
    },
    domain::stats::ActivityInsert,
    error::AppError,
    inbound::v1::responses::image_budget::{self, ImageBudgetOutcome, ImageBudgetPolicy},
    service::{
        recording::{record_stream_with_rollout, StreamRecording, StreamRollout, StreamSession},
        upstream_errors::upstream_failure_message,
    },
    store::storage::ResponseStateInsert,
    AppState,
};

use super::{
    activity::{record_upstream_failure, upstream_error_details, UpstreamFailure},
    candidate::ResponseBridgeContext,
};

pub(super) struct NativeResponseRequest<'a> {
    pub(super) payload: &'a Value,
    pub(super) request: &'a InternalRequest,
    pub(super) previous_response_id: Option<String>,
    pub(super) input_items: &'a [Value],
}

pub(super) async fn create_native_response(
    state: &AppState,
    request: NativeResponseRequest<'_>,
    model_upstream: &str,
    provider: crate::domain::registry::ProviderConfig,
    context: ResponseBridgeContext,
) -> Result<Response, AppError> {
    let NativeResponseRequest {
        payload,
        request,
        previous_response_id,
        input_items,
    } = request;
    let upstream_base_url = provider_upstream_base_url(&provider, UpstreamProtocol::Responses);
    let upstream_url = format!("{}/responses", upstream_base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let client = state.provider_client(&provider.provider_type)?;
    let image_budget_policy = image_budget::policy();
    let input_items = budget_input_items(input_items, image_budget_policy)?;
    let upstream_request_body =
        crate::codec::providers::responses::encode_native_responses_request_body(
            payload,
            model_upstream,
            Some(input_items.as_ref()),
        );
    let upstream_request = with_provider_headers(
        client
            .post(upstream_url)
            .bearer_auth(&provider.api_key)
            .header(header::CONTENT_TYPE, "application/json"),
        state
            .provider_catalog
            .provider_request_headers(&provider.provider_type),
        &RequestHeaderContext {
            session_id: Some(&context.session_id),
        },
    )
    .body(upstream_request_body);
    let upstream_response = match crate::outbound::client::send_with_timeout(
        crate::outbound::client::policy(),
        upstream_request,
    )
    .await
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
                    provider_type: provider.provider_type.clone(),
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
                &context.session,
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
            request_items: input_items.to_vec(),
            response_id: context.response_id.clone(),
        });
        let log = ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "responses".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            provider_type: provider.provider_type,
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
                        session: context.session.clone(),
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

    let mut response = match upstream_response.json::<Value>().await {
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
                },
            )
            .await;
            return Err(error);
        }
    };
    let upstream_response_id = response
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string);
    response["id"] = Value::String(context.response_id.clone());
    tracing::debug!(
        response_id = %context.response_id,
        upstream_response_id = upstream_response_id.as_deref(),
        "rewrote the native Responses identifier"
    );
    let decoded_response =
        match crate::codec::providers::responses::decode_responses_response(response.clone()) {
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
                    },
                )
                .await;
                return Err(error);
            }
        };
    if request.store {
        let request_items = Value::Array(input_items.to_vec());
        let response_items = response
            .get("output")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        state
            .storage
            .save_response_state(ResponseStateInsert {
                identity_id: &context.identity_id,
                response_id: &decoded_response.id,
                previous_response_id: previous_response_id.as_deref(),
                conversation_id: &context.conversation_id,
                provider_id: &provider.id,
                model: &decoded_response.model,
                upstream_response_id: upstream_response_id.as_deref(),
                request_items: &request_items,
                response_items: &response_items,
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
                provider_type: provider.provider_type,
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
            &context.session,
            events_from_internal_request(request),
            events_from_internal_response(&decoded_response),
        )
        .await?;

    Ok(Json(response).into_response())
}

pub(super) fn budget_input_items<'a>(
    input_items: &'a [Value],
    policy: &ImageBudgetPolicy,
) -> Result<Cow<'a, [Value]>, AppError> {
    if !policy.enabled {
        return Ok(Cow::Borrowed(input_items));
    }
    let budget_payload = json!({ "input": input_items });
    match image_budget::apply(&budget_payload, policy) {
        ImageBudgetOutcome::Disabled => Ok(Cow::Borrowed(input_items)),
        ImageBudgetOutcome::WithinBudget(report) => {
            tracing::debug!(
                total_bytes = report.total_bytes,
                max_bytes = policy.max_bytes,
                "image payload is within the upstream budget"
            );
            Ok(Cow::Borrowed(input_items))
        }
        ImageBudgetOutcome::Trimmed { payload, report } => {
            tracing::info!(
                total_bytes = report.total_bytes,
                remaining_bytes = report.remaining_bytes,
                dropped_images = report.dropped_images,
                max_bytes = policy.max_bytes,
                "trimmed historical images for the upstream image budget"
            );
            Ok(Cow::Owned(
                payload
                    .get("input")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            ))
        }
        ImageBudgetOutcome::CannotMeetBudget(report) => {
            tracing::warn!(
                total_bytes = report.total_bytes,
                remaining_bytes = report.remaining_bytes,
                dropped_images = report.dropped_images,
                max_bytes = policy.max_bytes,
                keep_last = policy.keep_last,
                "image payload cannot be reduced to the upstream budget"
            );
            Err(AppError::PayloadTooLarge(format!(
                "图片请求体超过上游预算：已丢弃 {} 张历史图片，仍有 {} 字节，预算 {} 字节。请减少图片，或调整 IMAGE_BUDGET_MAX_BYTES。",
                report.dropped_images, report.remaining_bytes, policy.max_bytes
            )))
        }
    }
}
