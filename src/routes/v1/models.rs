use axum::{
    extract::{Extension, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::{
    error::AppError,
    models::EndpointModel,
    providers::spec::{ProviderCapabilities, ProviderSpec, UpstreamProtocol},
    routes::v1::auth::CurrentProtocolAccess,
    AppState,
};

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
    reasoning: bool,
    tool_choice: bool,
    parallel_tool_calls: bool,
    system_messages: bool,
    structured_outputs: bool,
    streaming_usage: bool,
    max_context_tokens: Option<i64>,
    max_output_tokens: Option<i64>,
}

impl From<ProviderCapabilities> for ModelCapabilities {
    fn from(capabilities: ProviderCapabilities) -> Self {
        Self {
            tool_calls: capabilities.tool_calls,
            reasoning: capabilities.reasoning,
            tool_choice: capabilities.tool_choice,
            parallel_tool_calls: capabilities.parallel_tool_calls,
            system_messages: capabilities.system_messages,
            structured_outputs: capabilities.structured_outputs,
            streaming_usage: capabilities.streaming_usage,
            max_context_tokens: capabilities.max_context_tokens,
            max_output_tokens: capabilities.max_output_tokens,
        }
    }
}

async fn list_models(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
) -> Result<Json<ModelsResponse>, AppError> {
    let models = state
        .storage
        .list_protocol_models(&crate::storage::ProtocolAccess {
            identity_id: access.identity_id,
            endpoint_id: access.endpoint_id,
            endpoint_name: access.endpoint_name,
        })
        .await?;
    let mut seen_model_names = HashSet::new();
    let data = models
        .into_iter()
        .filter_map(|model| {
            if !seen_model_names.insert(model.model.model_name.clone()) {
                return None;
            }
            let mut providers = HashMap::new();
            providers.insert(
                model.provider.id.clone(),
                (
                    model.provider.name.clone(),
                    ProviderSpec::from_provider_config(&model.provider),
                ),
            );
            Some(model_entry_for_endpoint_model(model.model, &providers))
        })
        .collect::<Vec<_>>();

    Ok(Json(ModelsResponse {
        object: "list",
        data,
    }))
}

fn model_entry_for_endpoint_model(
    model: EndpointModel,
    providers_by_id: &HashMap<String, (String, ProviderSpec)>,
) -> ModelEntry {
    let provider = providers_by_id.get(&model.provider_id);
    let downstream_protocols = provider
        .map(|(_, spec)| downstream_protocols_for_spec(spec))
        .unwrap_or_default();

    ModelEntry {
        id: model.model_name,
        object: "model",
        entry_type: "endpoint_model",
        owned_by: "prelay",
        provider_id: model.provider_id,
        provider_name: provider
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| model.upstream_model.clone()),
        upstream_protocol: provider
            .map(|(_, spec)| spec)
            .map(|spec| protocol_name(spec.protocol))
            .unwrap_or("unknown"),
        upstream_model: model.upstream_model,
        downstream_protocols,
        capabilities: provider
            .map(|(_, spec)| spec)
            .map(|spec| spec.capabilities.into())
            .unwrap_or_else(|| ProviderCapabilities::limited().into()),
    }
}

fn protocol_name(protocol: UpstreamProtocol) -> &'static str {
    match protocol {
        UpstreamProtocol::Responses => "responses",
        UpstreamProtocol::ChatCompletions => "chat_completions",
        UpstreamProtocol::AnthropicMessages => "anthropic_messages",
    }
}

fn downstream_protocols_for_spec(spec: &ProviderSpec) -> Vec<String> {
    let mut values = Vec::new();
    for upstream_protocol in &spec.supported_protocols {
        for downstream_protocol in upstream_protocol.downstream_protocols() {
            if !values.contains(downstream_protocol) {
                values.push(*downstream_protocol);
            }
        }
    }
    values.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::State,
        http::{Request, StatusCode},
        middleware, Router,
    };
    use tower::ServiceExt;

    use super::list_models;
    use crate::routes::v1::endpoint_resolver::{create_test_endpoint_auth, test_provider};

    #[tokio::test]
    async fn lists_identity_endpoint_models_only() {
        let state = crate::test_support::test_state().await;
        let provider = test_provider(
            "DeepSeek",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-test",
        )
        .await
        .expect("create legacy provider source");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "coder", "deepseek-chat").await;

        let response = list_models(State(state), auth.access)
            .await
            .expect("list models");
        let model = response.0.data.first().expect("identity model listed");

        assert_eq!(response.0.object, "list");
        assert_eq!(response.0.data.len(), 1);
        assert_eq!(model.id, "coder");
        assert_eq!(model.object, "model");
        assert_eq!(model.entry_type, "endpoint_model");
        assert_eq!(model.owned_by, "prelay");
        assert_eq!(model.provider_name, "DeepSeek");
        assert_eq!(model.upstream_protocol, "chat_completions");
        assert_eq!(model.upstream_model, "deepseek-chat");
        assert_eq!(
            model.downstream_protocols,
            ["responses", "chat_completions", "anthropic_messages"]
        );
        assert!(model.capabilities.tool_calls);
    }

    #[tokio::test]
    async fn rejects_unauthenticated_models_request() {
        let state = crate::test_support::test_state().await;
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state,
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("route request");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
