use anyhow::Result;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;

use crate::{inbound, service::recording::RecordingHealthStatus, AppState};

pub async fn router(state: AppState) -> Result<Router> {
    let health_router = Router::new()
        .route("/health", get(health))
        .with_state(state.clone());
    Ok(Router::new()
        .merge(health_router)
        .nest("/api", inbound::api::router(state.clone()))
        .nest("/v1", inbound::v1::router(state))
        .fallback(not_found))
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let recording = state
        .recording
        .as_ref()
        .map(|recording| recording.health_snapshot());
    let status = recording
        .as_ref()
        .map_or(StatusCode::OK, |recording| match recording.status {
            RecordingHealthStatus::Blocked => StatusCode::SERVICE_UNAVAILABLE,
            RecordingHealthStatus::Ready | RecordingHealthStatus::Degraded => StatusCode::OK,
        });
    (
        status,
        Json(json!({
            "status": if status == StatusCode::OK { "ok" } else { "blocked" },
            "recording": recording,
        })),
    )
}

async fn not_found() -> impl IntoResponse {
    StatusCode::NOT_FOUND
}
