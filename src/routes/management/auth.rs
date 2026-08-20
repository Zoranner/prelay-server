use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};

use crate::{error::AppError, AppState};

#[derive(Clone, Debug)]
pub struct CurrentIdentity {
    pub id: String,
    pub credential_hash: String,
}

pub async fn require_device_credential(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let credential = extract_bearer_credential(request.headers()).ok_or(AppError::Unauthorized)?;
    let identity = state
        .storage
        .authenticate_identity(&credential)
        .await?
        .ok_or(AppError::Unauthorized)?;
    request.extensions_mut().insert(CurrentIdentity {
        id: identity.id,
        credential_hash: identity.credential_hash,
    });
    Ok(next.run(request).await)
}

fn extract_bearer_credential(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|credential| !credential.is_empty())
        .map(str::to_string)
}
