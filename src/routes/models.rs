use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::{db, error::AppError, models::ProviderConfig, AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/models", get(list_models))
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
    let data = configs
        .into_iter()
        .filter_map(model_entry_for_provider)
        .collect();

    Ok(Json(ModelsResponse {
        object: "list",
        data,
    }))
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
    use axum::extract::State;
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
}
