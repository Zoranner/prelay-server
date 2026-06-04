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
    models::{CreateModelAliasRequest, ModelAliasResponse},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/configs", get(list_configs).post(create_config))
        .route("/configs/by-token/:token", get(get_config_by_token))
        .route("/configs/:id", put(update_config).delete(delete_config))
        .route("/configs/:id/regenerate-token", post(regenerate_token))
        .route(
            "/model-aliases",
            get(list_model_aliases).post(create_model_alias),
        )
}

async fn get_config_by_token(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let config = db::get_config_by_token(&state.db, &token)
        .await?
        .ok_or_else(|| AppError::NotFound("密钥不存在".to_string()))?;
    Ok(Json(ConfigResponse::from(config)))
}

async fn list_configs(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let configs = db::list_configs(&state.db).await?;
    let responses: Vec<ConfigResponse> = configs.into_iter().map(Into::into).collect();
    Ok(Json(responses))
}

async fn list_model_aliases(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let aliases = db::list_model_aliases(&state.db).await?;
    let responses: Vec<ModelAliasResponse> = aliases.into_iter().map(Into::into).collect();
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

async fn create_model_alias(
    State(state): State<AppState>,
    Json(req): Json<CreateModelAliasRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.alias.trim().is_empty() {
        return Err(AppError::BadRequest("别名不能为空".to_string()));
    }
    if req.provider_id.trim().is_empty() {
        return Err(AppError::BadRequest("Provider 不能为空".to_string()));
    }
    if req.upstream_model.trim().is_empty() {
        return Err(AppError::BadRequest("上游模型不能为空".to_string()));
    }
    let protocols = req
        .downstream_protocols
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let alias = db::create_model_alias(
        &state.db,
        &req.alias,
        &req.provider_id,
        &req.upstream_model,
        &protocols,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(ModelAliasResponse::from(alias))))
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    use crate::{db, AppState};

    #[tokio::test]
    async fn creates_model_alias_from_admin_api() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };
        let app = Router::new().nest("/api", super::router().with_state(state.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/model-aliases"))
            .json(&json!({
                "alias": "coder",
                "provider_id": provider.id,
                "upstream_model": "deepseek-chat",
                "downstream_protocols": ["responses", "chat_completions"]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::CREATED);

        let resolved = db::get_provider_by_model(&state.db, "coder")
            .await
            .expect("resolve alias")
            .expect("alias exists");
        assert_eq!(resolved.model_upstream, "deepseek-chat");

        server.abort();
    }

    #[tokio::test]
    async fn lists_model_aliases_from_admin_api() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        db::create_model_alias(&db, "coder", &provider.id, "deepseek-chat", &["responses"])
            .await
            .expect("create alias");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .get(format!("http://{addr}/api/model-aliases"))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let aliases: serde_json::Value = response.json().await.expect("parse aliases json");
        assert_eq!(aliases[0]["alias"], "coder");
        assert_eq!(aliases[0]["provider_id"], provider.id);
        assert_eq!(aliases[0]["upstream_model"], "deepseek-chat");

        server.abort();
    }
}
