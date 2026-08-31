use axum::{
    extract::{Extension, State},
    response::Response,
    Json,
};
use serde_json::Value;

use crate::{
    bridge::anthropic::decode::decode_anthropic_request,
    error::AppError,
    routes::v1::{
        auth::CurrentProtocolAccess, endpoint_resolver::resolve_endpoint_model_candidates,
    },
    AppState,
};

use super::candidate::create_message_with_candidate;

pub(super) struct AnthropicMessageCandidateRequest {
    pub(super) original_payload: Value,
    pub(super) request: crate::bridge::internal::InternalRequest,
    pub(super) model_requested: String,
    pub(super) is_streaming: bool,
    pub(super) started_at: std::time::Instant,
}

pub(super) async fn create_message(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let original_payload = payload.clone();
    let request = decode_anthropic_request(payload)?;
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let candidates =
        resolve_endpoint_model_candidates(&state, &access, &request.model, "anthropic_messages")
            .await?;
    let mut last_upstream_error = None;
    for resolved in candidates.into_iter().take(
        crate::upstream::policy()
            .max_candidates
            .unwrap_or(usize::MAX),
    ) {
        let provider_id = resolved.provider.id.clone();
        match crate::upstream::retry_with_policy(crate::upstream::policy(), || {
            create_message_with_candidate(
                &state,
                &access,
                AnthropicMessageCandidateRequest {
                    original_payload: original_payload.clone(),
                    request: request.clone(),
                    model_requested: model_requested.clone(),
                    is_streaming,
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
                        &request.model,
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
        .unwrap_or_else(|| AppError::BadRequest(format!("接入点未配置模型 {}", request.model))))
}
