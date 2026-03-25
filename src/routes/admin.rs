use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};

use crate::{
    db,
    error::AppError,
    models::{ConfigResponse, CreateConfigRequest, UpdateConfigRequest},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/configs", get(list_configs).post(create_config))
        .route("/configs/:id", put(update_config))
        .route("/configs/:id", axum::routing::delete(delete_config))
        .route("/configs/:id/regenerate-token", post(regenerate_token))
}

async fn list_configs(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let configs = db::list_configs(&state.db).await?;
    let responses: Vec<ConfigResponse> = configs.into_iter().map(Into::into).collect();
    Ok(Json(responses))
}

async fn create_config(
    State(state): State<AppState>,
    Json(req): Json<CreateConfigRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("名称不能为空".to_string()));
    }
    if req.api_key.trim().is_empty() {
        return Err(AppError::BadRequest("API Key 不能为空".to_string()));
    }
    if req.base_url.trim().is_empty() {
        return Err(AppError::BadRequest("Base URL 不能为空".to_string()));
    }

    let config = db::create_config(
        &state.db,
        &req.name,
        &req.provider_type,
        &req.base_url,
        &req.api_key,
    )
    .await?;

    let response: ConfigResponse = config.into();
    Ok((StatusCode::CREATED, Json(response)))
}

async fn update_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<impl IntoResponse, AppError> {
    let updated = db::update_config(
        &state.db,
        &id,
        req.name.as_deref(),
        req.provider_type.as_deref(),
        req.base_url.as_deref(),
        req.api_key.as_deref(),
    )
    .await?;

    if !updated {
        return Err(AppError::NotFound(format!("配置 {} 不存在", id)));
    }

    let config = db::get_config_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    Ok(Json(ConfigResponse::from(config)))
}

async fn delete_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = db::delete_config(&state.db, &id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("配置 {} 不存在", id)));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn regenerate_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let token = db::regenerate_token(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    Ok(Json(serde_json::json!({ "token": token })))
}
