use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use prelay_protocol::{CreateEndpointRequest, EndpointResponse, UpdateEndpointRequest};

use crate::{error::AppError, AppState};

use super::auth::CurrentIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/endpoints", get(list).post(create))
        .route(
            "/endpoints/:endpoint_id",
            get(get_one).patch(update).delete(delete_one),
        )
        .route(
            "/endpoints/:endpoint_id/regenerate-token",
            post(regenerate_token),
        )
}

async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<Vec<EndpointResponse>>, AppError> {
    Ok(Json(
        state
            .storage
            .list_endpoints_with_catalog(&identity.id, &state.provider_catalog)
            .await?,
    ))
}

async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Json(input): Json<CreateEndpointRequest>,
) -> Result<(StatusCode, Json<EndpointResponse>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .storage
                .create_interface_with_catalog(&identity.id, input, &state.provider_catalog)
                .await?,
        ),
    ))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<EndpointResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .get_interface_with_catalog(&identity.id, &endpoint_id, &state.provider_catalog)
            .await?,
    ))
}

async fn update(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(endpoint_id): Path<String>,
    Json(input): Json<UpdateEndpointRequest>,
) -> Result<Json<EndpointResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .update_interface_with_catalog(
                &identity.id,
                &endpoint_id,
                input,
                &state.provider_catalog,
            )
            .await?,
    ))
}

async fn delete_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(endpoint_id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .storage
        .delete_interface(&identity.id, &endpoint_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn regenerate_token(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(endpoint_id): Path<String>,
) -> Result<Json<EndpointResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .regenerate_endpoint_token_with_catalog(
                &identity.id,
                &endpoint_id,
                &state.provider_catalog,
            )
            .await?,
    ))
}
