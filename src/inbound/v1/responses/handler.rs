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
        auth::CurrentProtocolAccess,
        candidates::{hold_reservation, run_endpoint_model_candidates},
        endpoint_resolver::resolve_language_model_candidates,
    },
    service::recording::record_provider_observation_best_effort,
    store::storage::{ProtocolAccess, ResponseState},
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
    pub(super) started_at_wall: chrono::DateTime<chrono::Utc>,
}

pub(super) async fn create_response(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let started_at_wall = chrono::Utc::now();
    let request = Arc::new(decode_responses_request(&payload)?);
    let payload = Arc::new(payload);
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let response_id = format!("resp_{}", uuid::Uuid::new_v4().simple());
    let previous_response_id = request.previous_response_id.clone();
    let parent_state = match previous_response_id.as_deref() {
        Some(previous_id) => {
            let pending = state.recording.as_ref().and_then(|recording| {
                recording.pending_response_state(&access.identity_id, previous_id)
            });
            match pending {
                Some(state) => Some(Arc::new(state)),
                None => state
                    .storage
                    .load_response_state(&access.identity_id, previous_id)
                    .await?
                    .map(Arc::new),
            }
        }
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
    let estimated_tokens = crate::service::quota::estimate_tokens(payload.as_ref());
    let quota = state.quota.clone();
    let storage = state.storage.clone();
    let identity_id = access.identity_id.clone();
    let (response, provider_id, reservation) = run_endpoint_model_candidates(
        candidates,
        &state.failover,
        AppError::BadRequest(format!("接入点未配置模型 {model_requested}")),
        move |resolved| {
            let quota = quota.clone();
            let storage = storage.clone();
            let identity_id = identity_id.clone();
            let provider_id = resolved.provider.id.clone();
            async move {
                quota
                    .reserve(&storage, &provider_id, &identity_id, estimated_tokens)
                    .await
            }
        },
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
                    started_at_wall,
                },
                resolved.clone(),
            )
        },
    )
    .await?;
    record_provider_observation_best_effort(
        &state,
        &ProtocolAccess {
            identity_id: access.identity_id.clone(),
            endpoint_id: access.endpoint_id.clone(),
            endpoint_name: access.endpoint_name.clone(),
        },
        &model_requested,
        &provider_id,
    )
    .await;
    Ok(hold_reservation(
        response,
        reservation,
        state.recording.is_some(),
    ))
}
