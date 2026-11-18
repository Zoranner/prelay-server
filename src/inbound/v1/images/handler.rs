use axum::{
    extract::{Extension, State},
    response::Response,
    Json,
};
use serde_json::Value;

use crate::{
    error::AppError,
    inbound::v1::{
        auth::CurrentProtocolAccess,
        candidates::{hold_reservation, run_endpoint_model_candidates},
        endpoint_resolver::resolve_endpoint_model_candidates,
    },
    service::recording::record_provider_observation_best_effort,
    store::storage::ProtocolAccess,
    AppState,
};

use super::{
    candidate::{create_image_generation_with_candidate, ImageCandidateRequest},
    IMAGE_GENERATIONS_PROTOCOL,
};

pub(super) async fn create_image_generation(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let started_at_wall = chrono::Utc::now();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("model 不能为空".to_string()))?
        .to_string();
    let is_streaming = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let session_id = access
        .session_id
        .clone()
        .unwrap_or_else(|| format!("conv_{}", uuid::Uuid::new_v4().simple()));
    let session = access.session_context(&session_id);
    let candidate = ImageCandidateRequest {
        session_id,
        session,
        model: model.clone(),
        is_streaming,
        started_at,
        started_at_wall,
    };
    let candidates =
        resolve_endpoint_model_candidates(&state, &access, &model, IMAGE_GENERATIONS_PROTOCOL)
            .await?;
    let estimated_tokens = crate::service::quota::estimate_tokens(&payload);
    let quota = state.quota.clone();
    let storage = state.storage.clone();
    let identity_id = access.identity_id.clone();
    let (response, provider_id, reservation) = run_endpoint_model_candidates(
        candidates,
        &state.failover,
        AppError::BadRequest(format!("接入点未配置模型 {model}")),
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
            create_image_generation_with_candidate(
                &state,
                &access,
                payload.clone(),
                candidate.clone(),
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
        &model,
        &provider_id,
    )
    .await;
    Ok(hold_reservation(response, reservation))
}
