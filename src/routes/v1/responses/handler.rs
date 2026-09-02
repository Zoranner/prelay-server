use axum::{
    extract::{Extension, State},
    response::Response,
    Json,
};
use serde_json::Value;

use crate::{
    bridge::{internal::InternalRequest, responses::decode::decode_responses_request},
    error::AppError,
    routes::v1::{
        auth::CurrentProtocolAccess, candidates::run_endpoint_model_candidates,
        endpoint_resolver::resolve_language_model_candidates,
    },
    AppState,
};

use super::candidate::create_response_with_candidate;

pub(super) struct ResponseCandidateRequest {
    pub(super) original_payload: Value,
    pub(super) request: InternalRequest,
    pub(super) model_requested: String,
    pub(super) is_streaming: bool,
    pub(super) previous_response_id: Option<String>,
    pub(super) started_at: std::time::Instant,
}

pub(super) async fn create_response(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let original_payload = payload.clone();
    let request = decode_responses_request(payload)?;
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let previous_response_id = request.previous_response_id.clone();
    let candidates =
        resolve_language_model_candidates(&state, &access, &request.model, "responses").await?;
    let (response, provider_id) = run_endpoint_model_candidates(
        candidates,
        AppError::BadRequest(format!("接入点未配置模型 {model_requested}")),
        |resolved| {
            create_response_with_candidate(
                &state,
                &access,
                ResponseCandidateRequest {
                    original_payload: original_payload.clone(),
                    request: request.clone(),
                    model_requested: model_requested.clone(),
                    is_streaming,
                    previous_response_id: previous_response_id.clone(),
                    started_at,
                },
                resolved.clone(),
            )
        },
    )
    .await?;
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
    Ok(response)
}
