use axum::{
    body::Body,
    extract::{Extension, State},
    http::header,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    bridge::{
        internal::InternalRequest,
        responses_decode::decode_responses_request_with_diagnostics,
        responses_encode::encode_responses_response,
        stream::{
            anthropic_messages_sse_response_to_responses_sse_with_stats,
            chat_sse_response_to_responses_sse_with_stats, native_responses_sse_with_stats,
        },
    },
    error::AppError,
    observability::{request_metadata::build_request_metadata, stream_stats::record_stream},
    providers::anthropic_messages::{
        decode_anthropic_messages_response, encode_anthropic_messages_request,
    },
    providers::chat_completions::{decode_chat_response, encode_chat_request},
    providers::responses::encode_responses_request,
    providers::spec::{provider_upstream_base_url, UpstreamProtocol},
    routes::v1::auth::CurrentProtocolAccess,
    routes::v1::endpoint_resolver::{resolve_endpoint_model_candidates, ResolvedEndpointProvider},
    stats::RequestLogInsert,
    storage::{ResponseSessionInsert, Storage},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/responses", post(create_response))
}

struct ResponseCandidateRequest {
    original_payload: Value,
    request: InternalRequest,
    diagnostics: Vec<crate::bridge::diagnostics::BridgeDiagnostic>,
    model_requested: String,
    is_streaming: bool,
    previous_response_id: Option<String>,
    started_at: std::time::Instant,
}

async fn create_response(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let original_payload = payload.clone();
    let decoded_request = decode_responses_request_with_diagnostics(payload)?;
    let request = decoded_request.request;
    let diagnostics = decoded_request.diagnostics;
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let previous_response_id = request.previous_response_id.clone();
    let candidates =
        resolve_endpoint_model_candidates(&state, &access, &request.model, "responses").await?;
    let mut last_upstream_error = None;
    for resolved in candidates.into_iter().take(
        crate::upstream::policy()
            .max_candidates
            .unwrap_or(usize::MAX),
    ) {
        let provider_id = resolved.provider.id.clone();
        match crate::upstream::retry_with_policy(crate::upstream::policy(), || {
            create_response_with_candidate(
                &state,
                &access,
                ResponseCandidateRequest {
                    original_payload: original_payload.clone(),
                    request: request.clone(),
                    diagnostics: diagnostics.clone(),
                    model_requested: model_requested.clone(),
                    is_streaming,
                    previous_response_id: previous_response_id.clone(),
                    started_at,
                },
                resolved.clone(),
            )
        })
        .await
        {
            Ok(response) => {
                if let Err(error) = state
                    .storage
                    .remember_protocol_model_provider(
                        &crate::storage::ProtocolAccess {
                            identity_id: access.identity_id.clone(),
                            endpoint_id: access.endpoint_id.clone(),
                            endpoint_name: access.endpoint_name.clone(),
                        },
                        &model_requested,
                        &provider_id,
                    )
                    .await
                {
                    tracing::warn!(error = %error, "failed to remember active endpoint model provider");
                }
                return Ok(response);
            }
            Err(error) if error.is_retryable_upstream() => last_upstream_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_upstream_error
        .unwrap_or_else(|| AppError::BadRequest(format!("接入点未配置模型 {model_requested}"))))
}

async fn create_response_with_candidate(
    state: &AppState,
    access: &CurrentProtocolAccess,
    candidate_request: ResponseCandidateRequest,
    resolved: ResolvedEndpointProvider,
) -> Result<Response, AppError> {
    let ResponseCandidateRequest {
        original_payload,
        request,
        diagnostics,
        model_requested,
        is_streaming,
        previous_response_id,
        started_at,
    } = candidate_request;
    let provider = resolved.provider;
    let upstream_protocol = resolved.upstream_protocol;
    let model_upstream = resolved.model_upstream;
    let metadata_json = build_request_metadata(diagnostics)?;
    let mut upstream_payload = original_payload;
    upstream_payload["model"] = Value::String(model_upstream.clone());
    let mut request = request;
    request.model = model_upstream.clone();
    if upstream_protocol == UpstreamProtocol::Responses {
        let (upstream_payload, request) = if previous_response_id.is_some() {
            let request =
                request_with_session_history(&state.storage, &access.identity_id, request).await?;
            (encode_responses_request(&request), request)
        } else {
            (upstream_payload, request)
        };
        return create_native_response(
            state,
            upstream_payload,
            provider,
            request,
            previous_response_id,
            ResponseBridgeContext {
                identity_id: access.identity_id.clone(),
                endpoint_name: access.endpoint_name.clone(),
                model_requested,
                is_streaming,
                metadata_json,
                started_at,
            },
        )
        .await;
    }
    if upstream_protocol == UpstreamProtocol::AnthropicMessages {
        return create_anthropic_messages_response(
            state,
            request,
            provider,
            previous_response_id,
            ResponseBridgeContext {
                identity_id: access.identity_id.clone(),
                endpoint_name: access.endpoint_name.clone(),
                model_requested,
                is_streaming,
                metadata_json,
                started_at,
            },
        )
        .await;
    }

    let request =
        request_with_session_history(&state.storage, &access.identity_id, request).await?;
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
        state
            .storage
            .insert_request_log(
                &access.identity_id,
                RequestLogInsert {
                    protocol_in: "responses".to_string(),
                    protocol_out: "responses".to_string(),
                    protocol_upstream: "chat_completions".to_string(),
                    provider_id: provider.id,
                    provider_name: provider.name,
                    endpoint_name: access.endpoint_name.clone(),
                    model_requested,
                    model_upstream: request.model,
                    status: "failed".to_string(),
                    http_status: status.as_u16() as i64,
                    error_code: None,
                    error_message: None,
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
                    upstream_request_id: None,
                    metadata_json: metadata_json.clone(),
                },
            )
            .await?;
        return Err(AppError::Upstream {
            status: Some(status),
            message: format!("上游请求失败: {status}"),
        });
    }

    if is_streaming {
        let log = RequestLogInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: access.endpoint_name.clone(),
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
            metadata_json: metadata_json.clone(),
        };
        let (stream, stream_stats) =
            chat_sse_response_to_responses_sse_with_stats(upstream_response);
        let body = Body::from_stream(record_stream(
            state.storage.clone(),
            access.identity_id.clone(),
            stream,
            log,
            started_at,
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
    let mut response = decode_chat_response(upstream_json)?;
    response.id = format!("resp_{}", Uuid::new_v4().simple());
    let reasoning_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| usage.reasoning_tokens);
    let tool_call_count = count_tool_calls(&response);
    state
        .storage
        .save_response_session(ResponseSessionInsert {
            identity_id: &access.identity_id,
            response_id: &response.id,
            previous_response_id: previous_response_id.as_deref(),
            provider_id: &provider.id,
            model: &response.model,
            input_messages: &request.messages,
            response: &response,
        })
        .await?;
    state
        .storage
        .insert_request_log(
            &access.identity_id,
            RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                endpoint_name: access.endpoint_name.clone(),
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
                metadata_json,
            },
        )
        .await?;

    Ok(Json(encode_responses_response(response)).into_response())
}

async fn create_anthropic_messages_response(
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
            .insert_request_log(
                &context.identity_id,
                RequestLogInsert {
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
        let log = RequestLogInsert {
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
            metadata_json: context.metadata_json.clone(),
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
    state
        .storage
        .insert_request_log(
            &context.identity_id,
            RequestLogInsert {
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
                metadata_json: context.metadata_json,
            },
        )
        .await?;

    Ok(Json(encode_responses_response(response)).into_response())
}

struct ResponseBridgeContext {
    identity_id: String,
    endpoint_name: String,
    model_requested: String,
    is_streaming: bool,
    metadata_json: Option<String>,
    started_at: std::time::Instant,
}

async fn create_native_response(
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
            .insert_request_log(
                &context.identity_id,
                RequestLogInsert {
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
        let log = RequestLogInsert {
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
    state
        .storage
        .insert_request_log(
            &context.identity_id,
            RequestLogInsert {
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
        )
        .await?;

    Ok(Json(response).into_response())
}

async fn request_with_session_history(
    storage: &Storage,
    identity_id: &str,
    mut request: InternalRequest,
) -> Result<InternalRequest, AppError> {
    let Some(previous_response_id) = request.previous_response_id.as_deref() else {
        return Ok(request);
    };
    let Some(mut history) = storage
        .load_response_session_messages(identity_id, previous_response_id)
        .await?
    else {
        return Err(AppError::BadRequest(format!(
            "previous_response_id {previous_response_id} 不存在"
        )));
    };
    history.extend(request.messages);
    request.messages = history;
    Ok(request)
}

fn count_tool_calls(response: &crate::bridge::internal::InternalResponse) -> i64 {
    response
        .output
        .iter()
        .filter(|item| item.is_tool_call())
        .count() as i64
}

#[cfg(test)]
fn responses_sse_from_text_chunks(chunks: &[&str]) -> String {
    let mut output = String::new();
    for chunk in chunks {
        output.push_str(
            std::str::from_utf8(&crate::bridge::stream::responses_text_delta_sse(chunk))
                .expect("sse chunk is utf8"),
        );
    }
    output.push_str(
        std::str::from_utf8(&crate::bridge::stream::responses_completed_sse())
            .expect("sse chunk is utf8"),
    );
    output
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::{
        extract::State,
        http::{Request, StatusCode},
        middleware,
        response::IntoResponse,
        routing::post,
        Json, Router,
    };
    use bytes::Bytes;
    use futures::{StreamExt, TryStreamExt};
    use serde_json::json;
    use std::{convert::Infallible, time::Duration};
    use tokio::net::TcpListener;
    use tower::ServiceExt;

    use super::create_response;
    use super::responses_sse_from_text_chunks;
    use crate::routes::v1::endpoint_resolver::{
        create_empty_test_endpoint_auth, create_test_endpoint_auth,
        create_test_endpoint_auth_with_candidates, test_provider,
    };

    #[tokio::test]
    async fn rejects_unauthenticated_responses_request() {
        let state = crate::test_support::test_state().await;
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state,
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/responses")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("route request");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn fails_over_responses_to_a_healthy_candidate_and_keeps_using_it() {
        let failing_upstream = spawn_failing_chat_upstream().await;
        let healthy_upstream = spawn_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let primary = test_provider(
            "primary",
            "openai_compatible",
            &failing_upstream,
            "sk-primary",
        )
        .await
        .expect("create primary provider");
        let backup = test_provider(
            "backup",
            "openai_compatible",
            &healthy_upstream,
            "sk-backup",
        )
        .await
        .expect("create backup provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[primary, backup],
            "shared-model",
            "deepseek-chat",
        )
        .await;
        let payload = json!({ "model": "shared-model", "input": "hello" });

        create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(payload.clone()),
        )
        .await
        .expect("fall back to the healthy candidate");
        create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(payload),
        )
        .await
        .expect("keep using the last successful candidate");

        let logs = state
            .storage
            .list_request_logs(&auth.access.0.identity_id, 10)
            .await
            .expect("load request logs");
        assert_eq!(logs.len(), 3);
        assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
        assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
    }

    #[tokio::test]
    async fn rejects_response_when_model_is_not_configured() {
        let state = crate::test_support::test_state().await;
        let auth = create_empty_test_endpoint_auth(&state.storage).await;

        let error = create_response(
            State(state),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect_err("missing endpoint model should fail");

        assert!(format!("{error:?}").contains("接入点未配置支持 responses 的模型 deepseek-chat"));
    }

    #[tokio::test]
    async fn forwards_responses_request_to_chat_completions_upstream() {
        let upstream = spawn_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let response = create_response(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");

        let response = response_json(response).await;

        assert_eq!(response["object"], "response");
        assert_eq!(response["model"], "deepseek-chat");
        assert_eq!(
            response["output"][0]["content"][0]["text"],
            "upstream hello"
        );
        assert_eq!(response["usage"]["input_tokens"], 3);
        assert_eq!(response["usage"]["output_tokens"], 4);
    }

    #[tokio::test]
    async fn forwards_responses_request_to_chat_bridge_before_anthropic_for_multi_protocol_provider(
    ) {
        let upstream = spawn_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "kimi_coding_anthropic",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let response = create_response(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");
        let response = response_json(response).await;

        assert_eq!(response["object"], "response");
        assert_eq!(response["model"], "deepseek-chat");
        assert_eq!(
            response["output"][0]["content"][0]["text"],
            "upstream hello"
        );
    }

    #[tokio::test]
    async fn forwards_responses_request_to_native_upstream() {
        let upstream = spawn_native_responses_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
            .await
            .expect("create provider");
        let auth = create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;

        let response = create_response(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "gpt-4.1",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");
        let response = response_json(response).await;

        assert_eq!(response["id"], "resp_native");
        assert_eq!(response["object"], "response");
        assert_eq!(response["model"], "gpt-4.1");
        assert_eq!(
            response["output"][0]["content"][0]["text"],
            "native response"
        );
    }

    #[tokio::test]
    async fn forwards_non_streaming_responses_request_to_anthropic_messages_upstream() {
        let upstream = spawn_native_anthropic_messages_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "claude-sonnet",
            "anthropic_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "claude-sonnet", "claude-sonnet")
                .await;

        let response = create_response(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "claude-sonnet",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");
        let response = response_json(response).await;

        assert_eq!(response["object"], "response");
        assert_eq!(response["model"], "claude-sonnet");
        assert_eq!(
            response["output"][0]["content"][0]["text"],
            "anthropic hello"
        );
        assert_eq!(response["usage"]["input_tokens"], 3);
        assert_eq!(response["usage"]["output_tokens"], 4);
    }

    #[tokio::test]
    async fn streams_anthropic_messages_chunks_as_responses_sse() {
        let upstream = spawn_streaming_native_anthropic_messages_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "claude-sonnet",
            "anthropic_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "claude-sonnet", "claude-sonnet")
                .await;

        let response = create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "claude-sonnet",
                "input": "hello",
                "stream": true
            })),
        )
        .await
        .expect("create response");
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(content_type.starts_with("text/event-stream"));

        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read stream body");
        let body = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(body.contains("event: response.output_text.delta\ndata: hel\n\n"));
        assert!(body.contains("event: response.output_text.delta\ndata: lo\n\n"));
        assert!(body.contains("event: response.completed"));

        let log = state
            .storage
            .list_request_logs(&auth.access.0.identity_id, 1)
            .await
            .expect("load request log")
            .pop()
            .expect("request log");

        assert_eq!(log.protocol_upstream.as_deref(), Some("anthropic_messages"));
        assert_eq!(log.status, "success");
        assert_eq!(log.http_status, Some(200));
        assert_eq!(log.error_code, None);
        assert_eq!(log.error_message, None);
    }

    #[tokio::test]
    async fn records_successful_response_request_log() {
        let upstream = spawn_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let _response = create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");
        let logs = state
            .storage
            .list_request_logs(&auth.access.0.identity_id, 10)
            .await
            .expect("load identity request log totals");

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, "success");
        assert_eq!(logs[0].input_tokens, Some(3));
        assert_eq!(logs[0].output_tokens, Some(4));
    }

    #[tokio::test]
    async fn records_response_decode_diagnostics_in_request_metadata() {
        let upstream = spawn_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let _response = create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": [
                    {
                        "role": "planner",
                        "content": "hello"
                    }
                ]
            })),
        )
        .await
        .expect("create response");
        let metadata_json = state
            .storage
            .list_request_logs(&auth.access.0.identity_id, 1)
            .await
            .expect("load metadata")
            .pop()
            .and_then(|log| log.metadata_json);
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata_json.expect("metadata json")).expect("parse metadata");

        assert_eq!(metadata["schema"], "provider-relay.request_metadata.v2");
        assert_eq!(metadata["diagnostics"][0]["code"], "responses.role.unknown");
        assert_eq!(metadata["diagnostics"][0]["action"], "mapped");
        assert_eq!(metadata["diagnostics"][0]["severity"], "warning");
    }

    #[tokio::test]
    async fn records_failed_response_request_log_when_upstream_fails() {
        let upstream = spawn_failing_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect_err("upstream failure should fail");
        let logs = state
            .storage
            .list_request_logs(&auth.access.0.identity_id, 10)
            .await
            .expect("load identity request log totals");

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, "failed");
    }

    #[tokio::test]
    async fn returns_responses_sse_when_stream_is_true() {
        let upstream = spawn_streaming_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let response = create_response(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello",
                "stream": true
            })),
        )
        .await
        .expect("create response")
        .into_response();
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read body");
        let body = String::from_utf8(body.to_vec()).expect("utf8 body");

        assert!(content_type.starts_with("text/event-stream"));
        assert!(body.contains("event: response.output_text.delta"));
        assert!(body.contains("data: hel"));
        assert!(body.contains("data: lo"));
        assert!(body.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn streams_responses_sse_delta_before_upstream_finishes() {
        let upstream = spawn_delayed_streaming_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let started = std::time::Instant::now();
        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/responses"))
            .bearer_auth(&auth.token)
            .json(&json!({
                "model": "deepseek-chat",
                "input": "hello",
                "stream": true
            }))
            .send()
            .await
            .expect("send request");
        let mut stream = response.bytes_stream();
        let first = stream
            .next()
            .await
            .expect("first response chunk")
            .expect("first response chunk ok");
        let elapsed = started.elapsed();
        let first = String::from_utf8(first.to_vec()).expect("first chunk utf8");

        assert!(
            elapsed < Duration::from_millis(200),
            "first relay chunk arrived after {elapsed:?}: {first}"
        );
        assert!(first.contains("event: response.output_text.delta"));
        assert!(first.contains("data: hel"));

        server.abort();
    }

    #[tokio::test]
    async fn streams_native_responses_sse_without_waiting_for_upstream_done() {
        let upstream = spawn_streaming_native_responses_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
            .await
            .expect("create provider");
        let auth = create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/responses"))
            .bearer_auth(&auth.token)
            .json(&json!({
                "model": "gpt-4.1",
                "input": "hello",
                "stream": true
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let started = std::time::Instant::now();
        let mut stream = response.bytes_stream();
        let first = stream
            .next()
            .await
            .expect("first response chunk")
            .expect("first response chunk ok");
        let elapsed = started.elapsed();
        let first = String::from_utf8(first.to_vec()).expect("first chunk utf8");

        assert!(
            elapsed < Duration::from_millis(200),
            "first native response chunk arrived after {elapsed:?}: {first}"
        );
        assert!(first.contains("event: response.output_text.delta"));
        assert!(first.contains("data: hel"));

        stream
            .try_collect::<Vec<_>>()
            .await
            .expect("read remaining response stream");
        let log = state
            .storage
            .list_request_logs(&auth.access.0.identity_id, 1)
            .await
            .expect("load request log")
            .pop()
            .expect("request log");
        assert_eq!(log.input_tokens, Some(11));
        assert_eq!(log.output_tokens, Some(7));

        server.abort();
    }

    #[tokio::test]
    async fn prepends_previous_response_messages_to_upstream_chat_request() {
        let upstream = spawn_history_asserting_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let first = create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "first user"
            })),
        )
        .await
        .expect("create first response");
        let first = response_json(first).await;
        let first_id = first["id"].as_str().expect("first id");

        let second = create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "previous_response_id": first_id,
                "input": "second user"
            })),
        )
        .await
        .expect("create second response");
        let second = response_json(second).await;

        assert_eq!(
            second["output"][0]["content"][0]["text"],
            "history accepted"
        );
        let second_id = second["id"].as_str().expect("second id");

        let third = create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "previous_response_id": second_id,
                "input": "third user"
            })),
        )
        .await
        .expect("create third response");
        let third = response_json(third).await;

        assert_eq!(
            third["output"][0]["content"][0]["text"],
            "full history accepted"
        );
    }

    #[tokio::test]
    async fn bridges_function_tool_call_roundtrip() {
        let upstream = spawn_tool_roundtrip_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let first = create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "please read"
            })),
        )
        .await
        .expect("create first response");
        let first = response_json(first).await;
        let first_id = first["id"].as_str().expect("first id");
        assert_eq!(first["output"][0]["type"], "function_call");
        assert_eq!(first["output"][0]["call_id"], "call_1");
        assert_eq!(first["output"][0]["name"], "read_file");

        let second = create_response(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(json!({
                "model": "deepseek-chat",
                "previous_response_id": first_id,
                "input": [
                    {
                        "type": "function_call_output",
                        "call_id": "call_1",
                        "output": "file text"
                    }
                ]
            })),
        )
        .await
        .expect("create second response");
        let second = response_json(second).await;

        assert_eq!(second["output"][0]["content"][0]["text"], "tool accepted");

        let logs = state
            .storage
            .list_request_logs(&auth.access.0.identity_id, 10)
            .await
            .expect("load tool call request logs");
        assert_eq!(logs.len(), 2);
        assert!(logs.iter().all(|log| log.status == "success"));
    }

    #[tokio::test]
    async fn rejects_unknown_previous_response_id() {
        let upstream = spawn_chat_upstream().await;
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let error = create_response(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "previous_response_id": "resp_missing",
                "input": "second user"
            })),
        )
        .await
        .expect_err("unknown previous response id should fail");

        assert!(format!("{error:?}").contains("previous_response_id resp_missing 不存在"));
    }

    #[test]
    fn encodes_upstream_text_chunks_as_responses_sse_events() {
        let encoded = responses_sse_from_text_chunks(&["hel", "lo"]);

        assert!(encoded.contains("event: response.output_text.delta\ndata: hel\n\n"));
        assert!(encoded.contains("event: response.output_text.delta\ndata: lo\n\n"));
        assert!(encoded.contains("event: response.completed\ndata: {}\n\n"));
        assert!(encoded.ends_with("data: [DONE]\n\n"));
    }

    async fn spawn_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");
            Json(json!({
                "id": "chatcmpl_test",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "upstream hello"
                        }
                    }
                ],
                "usage": {
                    "prompt_tokens": 3,
                    "completion_tokens": 4
                }
            }))
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_streaming_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            use axum::response::IntoResponse;

            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["stream"], true);
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                 data: [DONE]\n\n",
            )
                .into_response()
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_delayed_streaming_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            use axum::response::IntoResponse;

            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["stream"], true);
            let stream = futures::stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                                  data: [DONE]\n\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream),
            )
                .into_response()
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_native_responses_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "gpt-4.1");
            assert_eq!(payload["input"], "hello");
            Json(json!({
                "id": "resp_native",
                "object": "response",
                "model": "gpt-4.1",
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "native response"
                            }
                        ]
                    }
                ],
                "usage": {
                    "input_tokens": 3,
                    "output_tokens": 4
                }
            }))
        }

        let app = Router::new().route("/responses", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_streaming_native_responses_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            use axum::response::IntoResponse;

            assert_eq!(payload["model"], "gpt-4.1");
            assert_eq!(payload["stream"], true);
            let stream = futures::stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"event: response.output_text.delta\ndata: hel\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"event: response.output_text.delta\ndata: lo\n\n\
                                  event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}}\n\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream),
            )
                .into_response()
        }

        let app = Router::new().route("/responses", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_native_anthropic_messages_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "claude-sonnet");
            assert_eq!(payload["stream"], false);
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");
            Json(json!({
                "id": "msg_native",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet",
                "content": [
                    { "type": "text", "text": "anthropic hello" }
                ],
                "usage": {
                    "input_tokens": 3,
                    "output_tokens": 4
                }
            }))
        }

        let app = Router::new().route("/messages", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_streaming_native_anthropic_messages_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            use axum::response::IntoResponse;

            assert_eq!(payload["model"], "claude-sonnet");
            assert_eq!(payload["stream"], true);
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");
            let stream = futures::stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_native_stream\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream),
            )
                .into_response()
        }

        let app = Router::new().route("/messages", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_history_asserting_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            let messages = payload["messages"].as_array().expect("messages");
            let content = match messages.len() {
                1 => {
                    assert_eq!(messages[0]["content"], "first user");
                    "first assistant"
                }
                3 => {
                    assert_eq!(messages[0]["role"], "user");
                    assert_eq!(messages[0]["content"], "first user");
                    assert_eq!(messages[1]["role"], "assistant");
                    assert_eq!(messages[1]["content"], "first assistant");
                    assert_eq!(messages[2]["role"], "user");
                    assert_eq!(messages[2]["content"], "second user");
                    "history accepted"
                }
                5 => {
                    assert_eq!(messages[0]["role"], "user");
                    assert_eq!(messages[0]["content"], "first user");
                    assert_eq!(messages[1]["role"], "assistant");
                    assert_eq!(messages[1]["content"], "first assistant");
                    assert_eq!(messages[2]["role"], "user");
                    assert_eq!(messages[2]["content"], "second user");
                    assert_eq!(messages[3]["role"], "assistant");
                    assert_eq!(messages[3]["content"], "history accepted");
                    assert_eq!(messages[4]["role"], "user");
                    assert_eq!(messages[4]["content"], "third user");
                    "full history accepted"
                }
                len => panic!("unexpected history length: {len}"),
            };

            Json(json!({
                "id": "chatcmpl_history",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": content
                        }
                    }
                ],
                "usage": {
                    "prompt_tokens": 3,
                    "completion_tokens": 4
                }
            }))
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_tool_roundtrip_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            let messages = payload["messages"].as_array().expect("messages");
            if messages.len() == 1 {
                assert_eq!(messages[0]["role"], "user");
                return Json(json!({
                    "id": "chatcmpl_tool",
                    "model": payload["model"],
                    "choices": [
                        {
                            "message": {
                                "role": "assistant",
                                "content": null,
                                "reasoning_content": "Need to inspect the file before answering.",
                                "tool_calls": [
                                    {
                                        "id": "call_1",
                                        "type": "function",
                                        "function": {
                                            "name": "read_file",
                                            "arguments": "{\"path\":\"Cargo.toml\"}"
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }));
            }

            assert_eq!(messages.len(), 3);
            assert_eq!(messages[0]["role"], "user");
            assert_eq!(messages[0]["content"], "please read");
            assert_eq!(messages[1]["role"], "assistant");
            assert_eq!(
                messages[1]["reasoning_content"],
                "Need to inspect the file before answering."
            );
            assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
            assert_eq!(messages[2]["role"], "tool");
            assert_eq!(messages[2]["tool_call_id"], "call_1");
            assert_eq!(messages[2]["content"], "file text");

            Json(json!({
                "id": "chatcmpl_tool_done",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "tool accepted"
                        }
                    }
                ]
            }))
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read body");
        serde_json::from_slice(&body).expect("json body")
    }

    async fn spawn_failing_chat_upstream() -> String {
        async fn handler() -> (axum::http::StatusCode, Json<serde_json::Value>) {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "message": "upstream failed" } })),
            )
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }
}
