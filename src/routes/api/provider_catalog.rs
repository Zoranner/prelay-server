use axum::{extract::State, routing::get, Json, Router};
use prelay_protocol::ProviderCatalogResponse;

use crate::{error::AppError, AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/provider-catalog", get(get_catalog))
}

async fn get_catalog(
    State(state): State<AppState>,
) -> Result<Json<ProviderCatalogResponse>, AppError> {
    Ok(Json(state.provider_catalog.response()))
}
