use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use prelay_protocol::{
    CreateIdentityRequest, CreateIdentityResponse, RotateCredentialRequest,
    RotateCredentialResponse,
};

use crate::{error::AppError, AppState};

use super::auth::{extract_display_name, CurrentIdentity};

pub async fn create_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateIdentityRequest>,
) -> Result<(StatusCode, Json<CreateIdentityResponse>), AppError> {
    let header_display_name = extract_display_name(&headers);
    let response = state
        .storage
        .register_identity_with_display_name(
            request.machine_id.trim(),
            request.account_sid.trim(),
            &request.credential,
            request
                .display_name
                .as_deref()
                .or(header_display_name.as_deref()),
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
