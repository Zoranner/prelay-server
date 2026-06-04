use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::{
    db,
    error::AppError,
    models::{ModelAlias, ProviderConfig},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/v1/models", get(list_models))
}

#[derive(Debug, Serialize)]
struct ModelsResponse {
    object: &'static str,
    data: Vec<ModelEntry>,
}

#[derive(Debug, Serialize)]
struct ModelEntry {
    id: String,
    object: &'static str,
    owned_by: &'static str,
    provider_id: String,
    provider_name: String,
    upstream_protocol: &'static str,
}

async fn list_models(State(state): State<AppState>) -> Result<Json<ModelsResponse>, AppError> {
    let configs = db::list_configs(&state.db).await?;
    let aliases = db::list_model_aliases(&state.db).await?;
    let mut data = configs
        .into_iter()
        .filter_map(model_entry_for_provider)
        .collect::<Vec<_>>();
    data.extend(aliases.into_iter().map(model_entry_for_alias));

    Ok(Json(ModelsResponse {
        object: "list",
        data,
    }))
}

fn model_entry_for_alias(alias: ModelAlias) -> ModelEntry {
    ModelEntry {
        id: alias.alias,
        object: "model",
        owned_by: "provider-relay",
        provider_id: alias.provider_id,
        provider_name: alias.upstream_model,
        upstream_protocol: "alias",
    }
}

fn model_entry_for_provider(provider: ProviderConfig) -> Option<ModelEntry> {
    if !matches!(
        provider.provider_type.as_str(),
        "openai" | "zhipu" | "minimax" | "openai_compatible"
    ) {
        return None;
    }

    Some(ModelEntry {
        id: provider.name.clone(),
        object: "model",
        owned_by: "provider-relay",
        provider_id: provider.id,
        provider_name: provider.name,
        upstream_protocol: "chat_completions",
    })
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, middleware, Router};
    use sqlx::sqlite::SqlitePoolOptions;

    use super::list_models;
    use crate::{db, AppState};

    #[tokio::test]
    async fn lists_chat_completion_providers_as_openai_models() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-test",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let response = list_models(State(state)).await.expect("list models");

        assert_eq!(response.0.object, "list");
        assert_eq!(response.0.data.len(), 1);
        assert_eq!(response.0.data[0].id, provider.name);
        assert_eq!(response.0.data[0].object, "model");
        assert_eq!(response.0.data[0].owned_by, "provider-relay");
        assert_eq!(response.0.data[0].provider_id, provider.id);
        assert_eq!(response.0.data[0].provider_name, provider.name);
        assert_eq!(response.0.data[0].upstream_protocol, "chat_completions");
    }

    #[tokio::test]
    async fn lists_model_aliases_as_openai_models() {
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
            "sk-test",
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

        let response = list_models(State(state)).await.expect("list models");

        assert!(response.0.data.iter().any(|model| model.id == "coder"
            && model.provider_id == provider.id
            && model.provider_name == "deepseek-chat"
            && model.upstream_protocol == "alias"));
    }

    #[tokio::test]
    async fn rejects_unauthenticated_models_request() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };
        let app = Router::new().merge(super::router().with_state(state.clone()).layer(
            middleware::from_fn_with_state(state, crate::routes::auth::require_protocol_auth),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::get(format!("http://{addr}/v1/models"))
            .await
            .expect("send request");

        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        server.abort();
    }
}
