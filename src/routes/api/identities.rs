use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use prelay_protocol::{
    CreateIdentityRequest, CreateIdentityResponse, RotateCredentialRequest,
    RotateCredentialResponse,
};
use serde::Serialize;

use crate::{error::AppError, AppState};

use super::auth::{extract_display_name, CurrentIdentity};

#[derive(Debug, Serialize)]
pub struct CurrentIdentityResponse {
    pub identity_id: String,
}

pub async fn current_identity(
    Extension(identity): Extension<CurrentIdentity>,
) -> Json<CurrentIdentityResponse> {
    Json(CurrentIdentityResponse {
        identity_id: identity.id,
    })
}

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

#[cfg(test)]
mod tests {
    use axum::extract::Extension;

    use super::{current_identity, CurrentIdentity};

    #[tokio::test]
    async fn current_identity_returns_only_identity_id() {
        let response = current_identity(Extension(CurrentIdentity {
            id: "identity-a".into(),
            credential_hash: "hash".into(),
        }))
        .await;

        assert_eq!(response.0.identity_id, "identity-a");
    }
}
