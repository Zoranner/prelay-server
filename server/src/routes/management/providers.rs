use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use provider_relay_protocol::{CreateProviderRequest, ProviderResponse, UpdateProviderRequest};

use crate::{error::AppError, AppState};

use super::auth::CurrentIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/providers", get(list).post(create))
        .route(
            "/providers/:provider_id",
            get(get_one).patch(update).delete(delete_one),
        )
}

async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<Vec<ProviderResponse>>, AppError> {
    Ok(Json(state.storage.list_providers(&identity.id).await?))
}

async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Json(input): Json<CreateProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), AppError> {
    let provider_id = state.storage.create_provider(&identity.id, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .storage
                .get_provider(&identity.id, &provider_id)
                .await?,
        ),
    ))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<Json<ProviderResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .get_provider(&identity.id, &provider_id)
            .await?,
    ))
}

async fn update(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
    Json(input): Json<UpdateProviderRequest>,
) -> Result<Json<ProviderResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .update_provider(&identity.id, &provider_id, input)
            .await?,
    ))
}

async fn delete_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .storage
        .delete_provider(&identity.id, &provider_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
