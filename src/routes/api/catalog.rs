use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use prelay_protocol::{
    CatalogImageGenerationModelResponse, CatalogLanguageModelResponse, CatalogProviderResponse,
    ProviderCatalogResponse,
};

use crate::{error::AppError, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/catalog", get(get_catalog))
        .route("/catalog/providers", get(list_providers))
        .route("/catalog/providers/:provider_id", get(get_provider))
        .route("/catalog/models/language", get(list_language_models))
        .route(
            "/catalog/models/language/:model_id",
            get(get_language_model),
        )
        .route(
            "/catalog/models/image-generation",
            get(list_image_generation_models),
        )
        .route(
            "/catalog/models/image-generation/:model_id",
            get(get_image_generation_model),
        )
}

async fn get_catalog(State(state): State<AppState>) -> Json<ProviderCatalogResponse> {
    Json(state.provider_catalog.response())
}

async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<CatalogProviderResponse>>, AppError> {
    Ok(Json(state.provider_catalog.providers()))
}

async fn get_provider(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<CatalogProviderResponse>, AppError> {
    state
        .provider_catalog
        .provider_response(&provider_id)
        .map(Json)
        .ok_or_else(|| AppError::NotFound(format!("供应商目录项不存在: {provider_id}")))
}

async fn list_language_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<CatalogLanguageModelResponse>>, AppError> {
    Ok(Json(state.provider_catalog.language_models()))
}

async fn get_language_model(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
) -> Result<Json<CatalogLanguageModelResponse>, AppError> {
    state
        .provider_catalog
        .language_model_response(&model_id)
        .map(Json)
        .ok_or_else(|| AppError::NotFound(format!("语言模型目录项不存在: {model_id}")))
}

async fn list_image_generation_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<CatalogImageGenerationModelResponse>>, AppError> {
    Ok(Json(state.provider_catalog.image_generation_models()))
}

async fn get_image_generation_model(
    State(state): State<AppState>,
    Path(model_id): Path<String>,
) -> Result<Json<CatalogImageGenerationModelResponse>, AppError> {
    state
        .provider_catalog
        .image_generation_model_response(&model_id)
        .map(Json)
        .ok_or_else(|| AppError::NotFound(format!("图像生成模型目录项不存在: {model_id}")))
}
