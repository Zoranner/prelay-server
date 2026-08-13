use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use sqlx::SqlitePool;

use crate::{db, error::AppError, AppState};

pub async fn require_protocol_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    authenticate_protocol_request(&state.db, request.headers()).await?;
    Ok(next.run(request).await)
}

pub async fn authenticate_protocol_request(
    db: &SqlitePool,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let token = extract_token(headers).ok_or(AppError::Unauthorized)?;
    db::get_interface_by_token(db, &token)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(())
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
