use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderValue},
    response::Response,
    routing::get,
    Json, Router,
};
use prelay_protocol::{ClientUpdateResponse, ClientUpdateTarget, ProtocolErrorCode};
use tokio_util::io::ReaderStream;

use crate::{client_update::CachedClientUpdate, error::AppError, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client-update", get(latest))
        .route("/client-update/download", get(download))
}

async fn latest(
    State(state): State<AppState>,
    Query(target): Query<ClientUpdateTarget>,
) -> Result<Json<ClientUpdateResponse>, AppError> {
    let update = cached_update(&state, &target).await?;
    let file_name = update.file_name().to_string();
    Ok(Json(ClientUpdateResponse {
        version: update.version,
        file_name,
        download_path: format!(
            "/api/client-update/download?platform={}&architecture={}",
            target.platform, target.architecture
        ),
    }))
}

async fn download(
    State(state): State<AppState>,
    Query(target): Query<ClientUpdateTarget>,
) -> Result<Response, AppError> {
    let update = cached_update(&state, &target).await?;
    let path = update
        .installer_path(&state.client_update.cache_directory(), &target)
        .ok_or_else(unavailable_error)?;
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| unavailable_error())?;
    let metadata = file.metadata().await.map_err(|_| unavailable_error())?;
    let content_disposition =
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", update.file_name()))
            .map_err(|_| unavailable_error())?;

    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(|error| AppError::Internal(error.into()))
}

async fn cached_update(
    state: &AppState,
    target: &ClientUpdateTarget,
) -> Result<CachedClientUpdate, AppError> {
    state
        .client_update
        .latest(target)
        .await
        .ok_or_else(unavailable_error)
}

fn unavailable_error() -> AppError {
    AppError::Protocol {
        code: ProtocolErrorCode::ClientUpdateUnavailable,
        message: "客户端更新包暂不可用".to_string(),
    }
}
