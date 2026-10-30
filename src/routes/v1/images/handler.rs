use axum::{
    extract::{Extension, State},
    response::Response,
    Json,
};
use serde_json::Value;

use crate::{
    error::AppError,
    routes::v1::{
        auth::CurrentProtocolAccess, candidates::run_endpoint_model_candidates,
        endpoint_resolver::resolve_endpoint_model_candidates,
    },
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
    };
    let candidates =
        resolve_endpoint_model_candidates(&state, &access, &model, IMAGE_GENERATIONS_PROTOCOL)
            .await?;
    let (response, provider_id) = run_endpoint_model_candidates(
        candidates,
        &state.failover,
        AppError::BadRequest(format!("接入点未配置模型 {model}")),
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
    if let Err(error) = state
        .storage
        .remember_protocol_model_provider(
            &crate::store::storage::ProtocolAccess {
                identity_id: access.identity_id.clone(),
                endpoint_id: access.endpoint_id.clone(),
                endpoint_name: access.endpoint_name.clone(),
            },
            &model,
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
