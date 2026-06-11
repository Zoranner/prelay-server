use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use std::collections::HashMap;

use crate::{
    db,
    error::AppError,
    models::{ModelAlias, ProviderConfig},
    providers::spec::{ProviderCapabilities, ProviderSpec, UpstreamProtocol},
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
    entry_type: &'static str,
    owned_by: &'static str,
    provider_id: String,
    provider_name: String,
    upstream_protocol: &'static str,
    upstream_model: String,
    downstream_protocols: Vec<String>,
    capabilities: ModelCapabilities,
}

#[derive(Debug, Serialize)]
struct ModelCapabilities {
    tool_calls: bool,
}

impl From<ProviderCapabilities> for ModelCapabilities {
    fn from(capabilities: ProviderCapabilities) -> Self {
        Self {
            tool_calls: capabilities.tool_calls,
        }
    }
}

async fn list_models(State(state): State<AppState>) -> Result<Json<ModelsResponse>, AppError> {
    let configs = db::list_configs(&state.db).await?;
    let aliases = db::list_model_aliases(&state.db).await?;
    let providers_by_id = configs
        .iter()
        .map(|provider| {
            (
                provider.id.clone(),
                (
                    provider.name.clone(),
                    ProviderSpec::from_provider_config(provider),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut data = configs
        .into_iter()
        .map(model_entry_for_provider)
        .collect::<Vec<_>>();
    data.extend(
        aliases
            .into_iter()
            .map(|alias| model_entry_for_alias(alias, &providers_by_id)),
    );

    Ok(Json(ModelsResponse {
        object: "list",
        data,
    }))
}

fn model_entry_for_alias(
    alias: ModelAlias,
    providers_by_id: &HashMap<String, (String, ProviderSpec)>,
) -> ModelEntry {
    let provider = providers_by_id.get(&alias.provider_id);

    ModelEntry {
        id: alias.alias,
        object: "model",
        entry_type: "alias",
        owned_by: "provider-relay",
        provider_id: alias.provider_id,
        provider_name: provider
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| alias.upstream_model.clone()),
        upstream_protocol: provider
            .map(|(_, spec)| spec)
            .map(|spec| protocol_name(spec.protocol))
            .unwrap_or("alias"),
        upstream_model: alias.upstream_model,
        downstream_protocols: alias.downstream_protocols,
        capabilities: provider
            .map(|(_, spec)| spec)
            .map(|spec| spec.capabilities.into())
            .unwrap_or(ModelCapabilities { tool_calls: false }),
    }
}

fn model_entry_for_provider(provider: ProviderConfig) -> ModelEntry {
    let spec = ProviderSpec::from_provider_config(&provider);

    ModelEntry {
        id: provider.name.clone(),
        object: "model",
        entry_type: "provider",
        owned_by: "provider-relay",
        provider_id: provider.id,
        provider_name: provider.name.clone(),
        upstream_protocol: protocol_name(spec.protocol),
        upstream_model: provider.name,
        downstream_protocols: downstream_protocols_for_upstream(spec.protocol),
        capabilities: spec.capabilities.into(),
    }
}

fn protocol_name(protocol: UpstreamProtocol) -> &'static str {
    match protocol {
        UpstreamProtocol::Responses => "responses",
        UpstreamProtocol::ChatCompletions => "chat_completions",
        UpstreamProtocol::AnthropicMessages => "anthropic_messages",
        UpstreamProtocol::OllamaNative => "ollama_native",
    }
}

fn downstream_protocols_for_upstream(protocol: UpstreamProtocol) -> Vec<String> {
    match protocol {
        UpstreamProtocol::Responses => &["responses", "anthropic_messages"][..],
        UpstreamProtocol::ChatCompletions => {
            &["responses", "chat_completions", "anthropic_messages"][..]
        }
        UpstreamProtocol::AnthropicMessages => &["responses", "anthropic_messages"][..],
        UpstreamProtocol::OllamaNative => &[
            "responses",
            "chat_completions",
            "anthropic_messages",
            "ollama_native",
        ][..],
    }
    .iter()
    .map(|protocol| (*protocol).to_string())
    .collect()
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
            admin_token: None,
        };

        let response = list_models(State(state)).await.expect("list models");

        assert_eq!(response.0.object, "list");
        assert_eq!(response.0.data.len(), 1);
        assert_eq!(response.0.data[0].id, provider.name);
        assert_eq!(response.0.data[0].object, "model");
        assert_eq!(response.0.data[0].entry_type, "provider");
        assert_eq!(response.0.data[0].owned_by, "provider-relay");
        assert_eq!(response.0.data[0].provider_id, provider.id);
        assert_eq!(response.0.data[0].provider_name, provider.name);
        assert_eq!(response.0.data[0].upstream_protocol, "chat_completions");
        assert_eq!(
            response.0.data[0].downstream_protocols,
            ["responses", "chat_completions", "anthropic_messages"]
        );
        assert!(response.0.data[0].capabilities.tool_calls);
        assert_eq!(response.0.data[0].upstream_model, response.0.data[0].id);
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
            admin_token: None,
        };

        let response = list_models(State(state)).await.expect("list models");

        assert!(response.0.data.iter().any(|model| model.id == "coder"
            && model.provider_id == provider.id
            && model.entry_type == "alias"
            && model.provider_name == provider.name
            && model.upstream_protocol == "chat_completions"
            && model.upstream_model == "deepseek-chat"
            && model.downstream_protocols == ["responses"]));
    }

    #[tokio::test]
    async fn lists_native_responses_providers_as_openai_models() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider =
            db::create_config(&db, "OpenAI", "openai", "https://api.openai.com", "sk-test")
                .await
                .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };

        let response = list_models(State(state)).await.expect("list models");
        let model = response
            .0
            .data
            .iter()
            .find(|model| model.provider_id == provider.id)
            .expect("provider model listed");

        assert_eq!(model.id, provider.name);
        assert_eq!(model.upstream_protocol, "responses");
        assert_eq!(
            model.downstream_protocols,
            ["responses", "anthropic_messages"]
        );
        assert!(model.capabilities.tool_calls);
    }

    #[tokio::test]
    async fn lists_anthropic_compatible_providers_as_openai_models() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "Claude Relay",
            "anthropic_compatible",
            "https://anthropic.example.com",
            "sk-test",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };

        let response = list_models(State(state)).await.expect("list models");
        let model = response
            .0
            .data
            .iter()
            .find(|model| model.provider_id == provider.id)
            .expect("provider model listed");

        assert_eq!(model.id, provider.name);
        assert_eq!(model.upstream_protocol, "anthropic_messages");
        assert_eq!(
            model.downstream_protocols,
            ["responses", "anthropic_messages"]
        );
        assert!(model.capabilities.tool_calls);
    }

    #[tokio::test]
    async fn lists_ollama_native_providers_as_openai_models() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "Local Ollama",
            "ollama_native",
            "http://127.0.0.1:11434",
            "",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };

        let response = list_models(State(state)).await.expect("list models");
        let model = response
            .0
            .data
            .iter()
            .find(|model| model.provider_id == provider.id)
            .expect("provider model listed");

        assert_eq!(model.id, provider.name);
        assert_eq!(model.upstream_protocol, "ollama_native");
        assert_eq!(
            model.downstream_protocols,
            [
                "responses",
                "chat_completions",
                "anthropic_messages",
                "ollama_native"
            ]
        );
        assert!(!model.capabilities.tool_calls);
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
            admin_token: None,
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
