use axum::{body::Body, http::header, response::Response};
use futures::TryStreamExt;
use serde_json::Value;

use crate::{
    activity::{
        enqueue_activity_content_best_effort, media_metadata_from_bytes, RawStreamContentCapture,
        RawStreamProtocol,
    },
    error::AppError,
    observability::{
        stream_stats::record_first_chunk_with_activity_content,
        upstream_observability::upstream_observability,
    },
    providers::spec::provider_upstream_base_url,
    routes::v1::{auth::CurrentProtocolAccess, endpoint_resolver::ResolvedEndpointProvider},
    AppState,
};

use super::activity::{
    image_activity, insert_image_activity_best_effort, insert_image_activity_with_id_best_effort,
    ImageActivityParams,
};

pub(super) async fn create_image_generation_with_candidate(
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
        "{}/images/generations",
        upstream_base_url.trim_end_matches('/')
    );
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = match state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&payload)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            let safe_error_message = "上游连接失败".to_string();
            insert_image_activity_best_effort(
                state,
                access,
                image_activity(ImageActivityParams {
                    access,
                    provider: &provider,
                    model_requested: model,
                    model_upstream,
                    status: "failed",
                    http_status: 0,
                    error_code: Some("upstream_connection"),
                    is_streaming,
                    latency_ms: started_at.elapsed().as_millis() as i64,
                    upstream_latency_ms: None,
                    upstream_request_id: None,
                    error_message: Some(safe_error_message.clone()),
                }),
            )
            .await;
            return Err(AppError::Upstream {
                status: None,
                message: safe_error_message,
            });
        }
    };
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;
    let upstream_status = upstream_response.status();

    if !upstream_status.is_success() {
        let observability = upstream_observability(upstream_response.headers(), None);
        let safe_error_message = format!("上游请求失败: {upstream_status}");
        insert_image_activity_best_effort(
            state,
            access,
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
        let input_text = payload
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
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
        let body = Body::from_stream(record_first_chunk_with_activity_content(
            state.storage.clone(),
            access.identity_id.clone(),
            upstream_response
                .bytes_stream()
                .map_err(std::io::Error::other),
            log,
            started_at,
            input_text,
            RawStreamContentCapture::new(RawStreamProtocol::ImageGeneration),
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
            insert_image_activity_best_effort(
                state,
                access,
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
            )
            .await;
            return Err(AppError::Upstream {
                status: None,
                message: safe_error_message,
            });
        }
    };
    let input_text = payload
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let media_type = content_type
        .as_ref()
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");
    if let Some(activity_id) = insert_image_activity_with_id_best_effort(
        state,
        access,
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
    )
    .await
    {
        enqueue_activity_content_best_effort(
            &state.storage,
            activity_id,
            &input_text,
            "",
            Some(media_metadata_from_bytes(media_type, &response_bytes)),
        )
        .await;
    }

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
