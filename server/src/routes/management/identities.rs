use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use provider_relay_protocol::{
    CreateIdentityRequest, CreateIdentityResponse, RotateCredentialRequest,
    RotateCredentialResponse,
};

use crate::{error::AppError, AppState};

use super::auth::CurrentIdentity;

pub async fn create_identity(
    State(state): State<AppState>,
    Json(request): Json<CreateIdentityRequest>,
) -> Result<(StatusCode, Json<CreateIdentityResponse>), AppError> {
    let response = state
        .storage
        .register_identity(
            request.machine_id.trim(),
            request.account_sid.trim(),
            &request.credential,
        )
        .await?;
    let status = if response.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(response)))
}

pub async fn rotate_credential(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Json(request): Json<RotateCredentialRequest>,
) -> Result<Json<RotateCredentialResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .rotate_identity_credential(
                &identity.id,
                &identity.credential_hash,
                &request.new_credential,
            )
            .await?,
    ))
}
