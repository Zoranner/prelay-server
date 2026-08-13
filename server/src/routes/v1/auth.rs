use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};

use crate::{error::AppError, AppState};

#[derive(Clone, Debug)]
pub struct CurrentProtocolAccess {
    pub identity_id: String,
    pub interface_id: String,
}

pub async fn require_protocol_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let access = authenticate_protocol_request(&state, request.headers()).await?;
    let mut request = request;
    request.extensions_mut().insert(access);
    Ok(next.run(request).await)
}

pub async fn authenticate_protocol_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<CurrentProtocolAccess, AppError> {
    let token = extract_token(headers).ok_or(AppError::Unauthorized)?;
    state
        .storage
        .authenticate_protocol_access(&token)
        .await?
        .map(|access| CurrentProtocolAccess {
            identity_id: access.identity_id,
            interface_id: access.interface_id,
        })
        .ok_or(AppError::Unauthorized)
}

pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string)
        })
}
