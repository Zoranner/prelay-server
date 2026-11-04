use axum::{
    body::Body,
    http::{header, StatusCode},
    response::Response,
};
use futures::TryStreamExt;
use serde_json::Value;

use crate::{
    codec::activity::{
        conversation_events_from_image_request, conversation_events_from_image_response,
    },
    codec::bridge::stream::{upstream_bytes_stream, verify_upstream_sse_head, with_sse_keepalive},
    codec::providers::spec::{
        provider_upstream_base_url, with_provider_headers, RequestHeaderContext,
    },
    domain::activity::{RawStreamContentCapture, RawStreamProtocol, SessionContext},
    error::AppError,
    inbound::v1::{auth::CurrentProtocolAccess, endpoint_resolver::ResolvedEndpointProvider},
    service::{
        recording::{record_first_chunk_with_rollout_best_effort, StreamRollout},
        upstream_errors::upstream_observability,
    },
    AppState,
};

use super::activity::{image_activity, record_image_activity_best_effort, ImageActivityParams};

#[derive(Clone)]
pub(super) struct ImageCandidateRequest {
    pub(super) session_id: String,
    pub(super) session: SessionContext,
    pub(super) model: String,
    pub(super) is_streaming: bool,
    pub(super) started_at: std::time::Instant,
}

pub(super) async fn create_image_generation_with_candidate(
    state: &AppState,
    access: &CurrentProtocolAccess,
    mut payload: Value,
    candidate: ImageCandidateRequest,
    resolved: ResolvedEndpointProvider,
) -> Result<Response, AppError> {
    let ImageCandidateRequest {
        session_id,
        session,
        model,
        is_streaming,
        started_at,
    } = candidate;
    let provider = resolved.provider;
    let model_upstream = resolved.model_upstream;

    payload["model"] = Value::String(model_upstream.clone());
    let upstream_base_url = provider_upstream_base_url(&provider, resolved.upstream_protocol);
    let upstream_url = format!(
        "{}/images/generations",
        upstream_base_url.trim_end_matches('/')
    );
    let upstream_started_at = std::time::Instant::now();
    let client = state.provider_client(&provider.provider_type)?;
    let upstream_request = with_provider_headers(
        client.post(upstream_url).bearer_auth(&provider.api_key),
        state
            .provider_catalog
            .provider_request_headers(&provider.provider_type),
        &RequestHeaderContext {
            session_id: Some(&session_id),
        },
    )
    .json(&payload);
    let mut upstream_response = match crate::outbound::client::send_with_timeout(
        crate::outbound::client::policy(),
        upstream_request,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let (status, error_code, safe_error_message) = match &error {
                AppError::Upstream {
                    status: Some(StatusCode::GATEWAY_TIMEOUT),
                    ..
                } => (
                    Some(StatusCode::GATEWAY_TIMEOUT),
                    "upstream_response_head_timeout",
                    "上游响应超时",
                ),
                _ => (None, "upstream_connection", "上游连接失败"),
            };
            record_image_activity_best_effort(
                state,
                access,
                &session,
                image_activity(ImageActivityParams {
                    access,
                    provider: &provider,
                    model_requested: model,
                    model_upstream,
                    status: "failed",
                    http_status: status.map_or(0, |status| status.as_u16() as i64),
                    error_code: Some(error_code),
                    is_streaming,
                    latency_ms: started_at.elapsed().as_millis() as i64,
                    upstream_latency_ms: None,
                    upstream_request_id: None,
                    error_message: Some(safe_error_message.to_string()),
                }),
                conversation_events_from_image_request(&payload),
                Vec::new(),
            )
            .await;
            return Err(AppError::Upstream {
                status,
                message: safe_error_message.to_string(),
            });
        }
    };
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;
    let upstream_status = upstream_response.status();

    if !upstream_status.is_success() {
        let observability = upstream_observability(upstream_response.headers(), None);
        let safe_error_message = format!("上游请求失败: {upstream_status}");
        record_image_activity_best_effort(
            state,
            access,
            &session,
            image_activity(ImageActivityParams {
                access,
                provider: &provider,
                model_requested: model.clone(),
                model_upstream,
                status: "failed",
                http_status: upstream_status.as_u16() as i64,
                error_code: None,
                is_streaming,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                upstream_request_id: observability.request_id,
                error_message: Some(safe_error_message.clone()),
            }),
            conversation_events_from_image_request(&payload),
            Vec::new(),
        )
        .await;
        return Err(AppError::Upstream {
            status: Some(upstream_status),
            message: safe_error_message,
        });
    }

    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned();
    let upstream_request_id = upstream_observability(upstream_response.headers(), None).request_id;

    if is_streaming {
        let log = image_activity(ImageActivityParams {
            access,
            provider: &provider,
            model_requested: model,
            model_upstream,
            status: "success",
            http_status: upstream_status.as_u16() as i64,
            error_code: None,
            is_streaming: true,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            upstream_request_id,
            error_message: None,
        });
        let head = verify_upstream_sse_head(&mut upstream_response).await?;
        let stream = upstream_bytes_stream(upstream_response, head).map_err(std::io::Error::other);
        let body = Body::from_stream(with_sse_keepalive(
            record_first_chunk_with_rollout_best_effort(
                state.activity_store.clone(),
                access.identity_id.clone(),
                stream,
                log,
                started_at,
                RawStreamContentCapture::new(RawStreamProtocol::ImageGeneration),
                StreamRollout {
                    request_events: conversation_events_from_image_request(&payload),
                    session: session.clone(),
                },
            )
            .await,
        ));
        return Ok(upstream_response_with_body(
            upstream_status,
            content_type,
            body,
        ));
    }

    let response_bytes = match upstream_response.bytes().await {
        Ok(response_bytes) => response_bytes,
        Err(_) => {
            let safe_error_message = "读取上游响应失败".to_string();
            record_image_activity_best_effort(
                state,
                access,
                &session,
                image_activity(ImageActivityParams {
                    access,
                    provider: &provider,
                    model_requested: model,
                    model_upstream,
                    status: "failed",
                    http_status: upstream_status.as_u16() as i64,
                    error_code: Some("upstream_body"),
                    is_streaming: false,
                    latency_ms: started_at.elapsed().as_millis() as i64,
                    upstream_latency_ms: Some(upstream_latency_ms),
                    upstream_request_id,
                    error_message: Some(safe_error_message.clone()),
                }),
                conversation_events_from_image_request(&payload),
                Vec::new(),
            )
            .await;
            return Err(AppError::Upstream {
                status: None,
                message: safe_error_message,
            });
        }
    };
    record_image_activity_best_effort(
        state,
        access,
        &session,
        image_activity(ImageActivityParams {
            access,
            provider: &provider,
            model_requested: model,
            model_upstream,
            status: "success",
            http_status: upstream_status.as_u16() as i64,
            error_code: None,
            is_streaming: false,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            upstream_request_id,
            error_message: None,
        }),
        conversation_events_from_image_request(&payload),
        conversation_events_from_image_response(),
    )
    .await;

    Ok(upstream_response_with_body(
        upstream_status,
        content_type,
        Body::from(response_bytes),
    ))
}

fn upstream_response_with_body(
    status: reqwest::StatusCode,
    content_type: Option<reqwest::header::HeaderValue>,
    body: Body,
) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    response
}
