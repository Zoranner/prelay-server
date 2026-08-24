use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderValue},
    response::Response,
    routing::get,
    Json, Router,
};
use prelay_protocol::{ClientUpdateResponse, ProtocolErrorCode};
use tokio_util::io::ReaderStream;

use crate::{client_update::CachedClientUpdate, error::AppError, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client-update", get(latest))
        .route("/client-update/download", get(download))
}

async fn latest(State(state): State<AppState>) -> Result<Json<ClientUpdateResponse>, AppError> {
    let update = cached_update(&state).await?;
    Ok(Json(ClientUpdateResponse {
        version: update.version,
        download_path: "/api/client-update/download".to_string(),
    }))
}

async fn download(State(state): State<AppState>) -> Result<Response, AppError> {
    let update = cached_update(&state).await?;
    let path = update.installer_path(&state.client_update.cache_directory());
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| unavailable_error())?;
    let metadata = file.metadata().await.map_err(|_| unavailable_error())?;
    let content_disposition = HeaderValue::from_str(&format!(
        "attachment; filename=\"prelay-client-{}.exe\"",
        update.version
    ))
    .map_err(|_| unavailable_error())?;

    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "application/vnd.microsoft.portable-executable",
        )
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(|error| AppError::Internal(error.into()))
}

async fn cached_update(state: &AppState) -> Result<CachedClientUpdate, AppError> {
    state
        .client_update
        .latest()
        .await
        .ok_or_else(unavailable_error)
}

fn unavailable_error() -> AppError {
    AppError::Protocol {
        code: ProtocolErrorCode::ClientUpdateUnavailable,
        message: "客户端更新包暂不可用".to_string(),
    }
}
