use std::sync::Arc;

use axum::{
    extract::{Extension, State},
    response::Response,
    Json,
};
use serde_json::Value;

use crate::{
    codec::{bridge::responses::decode::decode_responses_request, internal::InternalRequest},
    error::AppError,
    inbound::v1::{
        auth::CurrentProtocolAccess, candidates::run_endpoint_model_candidates,
        endpoint_resolver::resolve_language_model_candidates,
    },
    store::storage::ResponseState,
    AppState,
};

use super::candidate::create_response_with_candidate;

pub(super) struct ResponseCandidateRequest {
    pub(super) payload: Arc<Value>,
    pub(super) request: Arc<InternalRequest>,
    pub(super) parent_state: Option<Arc<ResponseState>>,
    pub(super) response_id: String,
    pub(super) model_requested: String,
    pub(super) is_streaming: bool,
    pub(super) previous_response_id: Option<String>,
    pub(super) conversation_id: String,
    pub(super) started_at: std::time::Instant,
}

pub(super) async fn create_response(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let request = Arc::new(decode_responses_request(&payload)?);
    let payload = Arc::new(payload);
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let response_id = format!("resp_{}", uuid::Uuid::new_v4().simple());
    let previous_response_id = request.previous_response_id.clone();
    let parent_state = match previous_response_id.as_deref() {
        Some(response_id) => state
            .storage
            .load_response_state(&access.identity_id, response_id)
            .await?
            .map(Arc::new),
        None => None,
    };
    if previous_response_id.is_some() && parent_state.is_none() {
        return Err(AppError::openai_previous_response_not_found());
    }
    let conversation_id = parent_state
        .as_ref()
        .and_then(|parent| parent.conversation_id.clone())
        .unwrap_or_else(|| format!("conv_{}", uuid::Uuid::new_v4().simple()));
    let candidates =
        resolve_language_model_candidates(&state, &access, &request.model, "responses").await?;
    let (response, provider_id) = run_endpoint_model_candidates(
        candidates,
        &state.failover,
        AppError::BadRequest(format!("接入点未配置模型 {model_requested}")),
        |resolved| {
            create_response_with_candidate(
                &state,
                &access,
                ResponseCandidateRequest {
                    payload: Arc::clone(&payload),
                    request: Arc::clone(&request),
                    parent_state: parent_state.clone(),
                    response_id: response_id.clone(),
                    model_requested: model_requested.clone(),
                    is_streaming,
                    previous_response_id: previous_response_id.clone(),
                    conversation_id: conversation_id.clone(),
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
            &crate::store::storage::ProtocolAccess {
                identity_id: access.identity_id.clone(),
                endpoint_id: access.endpoint_id.clone(),
                endpoint_name: access.endpoint_name.clone(),
            },
            &model_requested,
            &provider_id,
        )
        .await
    {
        tracing::warn!(
            error_code = error.code().as_str(),
            "failed to remember active endpoint model provider"
        );
    }
    Ok(response)
}
