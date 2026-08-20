use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use prelay_protocol::{CreateInterfaceRequest, InterfaceResponse, UpdateInterfaceRequest};

use crate::{error::AppError, AppState};

use super::auth::CurrentIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/interfaces", get(list).post(create))
        .route(
            "/interfaces/:interface_id",
            get(get_one).patch(update).delete(delete_one),
        )
        .route(
            "/interfaces/:interface_id/regenerate-token",
            post(regenerate_token),
        )
}

async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<Vec<InterfaceResponse>>, AppError> {
    Ok(Json(state.storage.list_interfaces(&identity.id).await?))
}

async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Json(input): Json<CreateInterfaceRequest>,
) -> Result<(StatusCode, Json<InterfaceResponse>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(state.storage.create_interface(&identity.id, input).await?),
    ))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(interface_id): Path<String>,
) -> Result<Json<InterfaceResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .get_interface(&identity.id, &interface_id)
            .await?,
    ))
}

async fn update(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(interface_id): Path<String>,
    Json(input): Json<UpdateInterfaceRequest>,
) -> Result<Json<InterfaceResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .update_interface(&identity.id, &interface_id, input)
            .await?,
    ))
}

async fn delete_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(interface_id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .storage
        .delete_interface(&identity.id, &interface_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn regenerate_token(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(interface_id): Path<String>,
) -> Result<Json<InterfaceResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .regenerate_interface_token(&identity.id, &interface_id)
            .await?,
    ))
}
