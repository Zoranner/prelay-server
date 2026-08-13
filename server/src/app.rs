use anyhow::Result;
use axum::{http::StatusCode, response::IntoResponse, Router};

use crate::{routes, AppState};

pub async fn router(state: AppState) -> Result<Router> {
    Ok(Router::new()
        .nest("/api", routes::management::router(state.clone()))
        .nest("/v1", routes::v1::router(state))
        .fallback(not_found))
}

async fn not_found() -> impl IntoResponse {
    StatusCode::NOT_FOUND
}
