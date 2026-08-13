use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use provider_relay_protocol::{
    CreateIdentityRequest, CreateIdentityResponse, RotateCredentialResponse,
};

use crate::{error::AppError, AppState};

use super::auth::CurrentIdentity;

pub async fn create_identity(
    State(state): State<AppState>,
    Json(request): Json<CreateIdentityRequest>,
) -> Result<(StatusCode, Json<CreateIdentityResponse>), AppError> {
    let response = state
        .storage
        .register_identity(request.machine_id.trim(), request.account_sid.trim())
        .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn rotate_credential(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<RotateCredentialResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .rotate_identity_credential(&identity.id)
            .await?,
    ))
}
