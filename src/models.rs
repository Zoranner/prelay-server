use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    /// Stable provider template id, such as "openai", "kimi_coding", or "anthropic_compatible".
    pub provider_type: String,
    /// API base URL, e.g. https://api.openai.com
    pub base_url: String,
    pub api_key: String,
    /// Internal token used by clients to authenticate with this proxy
    pub token: String,
    pub capabilities_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ModelAlias {
    pub alias: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub downstream_protocols: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProviderModel {
    pub id: String,
    pub provider_id: String,
    pub model_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EndpointConfig {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub token: String,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EndpointModel {
    pub id: String,
    pub endpoint_id: String,
    pub model_name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateConfigRequest {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub capabilities: Option<ProviderCapabilityOverrides>,
    pub models: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub name: Option<String>,
    pub provider_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub capabilities: Option<ProviderCapabilityOverrides>,
    pub models: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct DiscoverModelsRequest {
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct TestProviderProtocolRequest {
    pub provider_type: String,
    pub protocol: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TestProviderProtocolResponse {
    pub ok: bool,
    pub protocol: String,
    pub latency_ms: i64,
    pub first_token_ms: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PingProviderResponse {
    pub ok: bool,
    pub latency_ms: i64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiscoverModelsResponse {
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilityOverrides {
    pub upstream_protocols: Option<Vec<String>>,
    pub protocol_base_urls: Option<ProviderProtocolBaseUrls>,
    pub tool_calls: Option<bool>,
    pub reasoning: Option<bool>,
    pub tool_choice: Option<bool>,
    pub parallel_tool_calls: Option<bool>,
    pub system_messages: Option<bool>,
    pub structured_outputs: Option<bool>,
    pub streaming_usage: Option<bool>,
    pub max_context_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderProtocolBaseUrls {
    pub responses: Option<String>,
    pub openai: Option<String>,
    pub anthropic: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateModelAliasRequest {
    pub alias: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub downstream_protocols: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderModelRequest {
    pub model_name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EndpointModelInput {
    pub provider_id: String,
    pub upstream_model: String,
    pub model_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEndpointRequest {
    pub name: String,
    pub protocol: Option<String>,
    pub models: Vec<EndpointModelInput>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEndpointRequest {
    pub name: Option<String>,
    pub models: Option<Vec<EndpointModelInput>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEndpointModelRequest {
    pub provider_id: String,
    pub upstream_model: String,
    pub model_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ModelAliasResponse {
    pub alias: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub downstream_protocols: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ProviderModelResponse {
    pub id: String,
    pub provider_id: String,
    pub model_name: String,
    pub created_at: String,
}

impl From<ProviderModel> for ProviderModelResponse {
    fn from(model: ProviderModel) -> Self {
        ProviderModelResponse {
            id: model.id,
            provider_id: model.provider_id,
            model_name: model.model_name,
            created_at: model.created_at,
        }
    }
}

impl From<ModelAlias> for ModelAliasResponse {
    fn from(alias: ModelAlias) -> Self {
        ModelAliasResponse {
            alias: alias.alias,
            provider_id: alias.provider_id,
            upstream_model: alias.upstream_model,
            downstream_protocols: alias.downstream_protocols,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EndpointModelResponse {
    pub id: String,
    pub endpoint_id: String,
    pub model_name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub created_at: String,
}

impl From<EndpointModel> for EndpointModelResponse {
    fn from(model: EndpointModel) -> Self {
        EndpointModelResponse {
            id: model.id,
            endpoint_id: model.endpoint_id,
            model_name: model.model_name,
            provider_id: model.provider_id,
            upstream_model: model.upstream_model,
            created_at: model.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EndpointResponse {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub token: String,
    pub models: Vec<EndpointModelResponse>,
    pub created_at: String,
}

impl EndpointResponse {
    pub fn from_config(config: EndpointConfig, models: Vec<EndpointModel>) -> Self {
        EndpointResponse {
            id: config.id,
            name: config.name,
            protocol: config.protocol,
            token: config.token,
            models: models.into_iter().map(Into::into).collect(),
            created_at: config.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    /// Masked API key, only first 4 and last 4 chars visible
    pub api_key_masked: String,
    pub token: String,
    pub capabilities: ProviderCapabilityOverrides,
    pub models: Vec<ProviderModelResponse>,
    pub created_at: String,
}

impl ProviderConfig {
    /// Returns true if this provider uses Anthropic-style auth
    /// (x-api-key + anthropic-version header) instead of Bearer.
    pub fn uses_anthropic_auth(&self) -> bool {
        matches!(
            self.provider_type.as_str(),
            "anthropic"
                | "anthropic_compatible"
                | "deepseek_anthropic"
                | "qwen_anthropic"
                | "zhipu_anthropic"
                | "minimax_anthropic"
                | "kimi_coding_anthropic"
                | "zai_coding_anthropic"
                | "zhipu_coding"
                | "minimax_token"
                | "bailian_coding_anthropic"
                | "bailian_token_anthropic"
        )
    }
}

impl From<ProviderConfig> for ConfigResponse {
    fn from(c: ProviderConfig) -> Self {
        ConfigResponse::from_config(c, Vec::new())
    }
}

impl ConfigResponse {
    pub fn from_config(c: ProviderConfig, models: Vec<ProviderModel>) -> Self {
        let api_key_masked = mask_key(&c.api_key);
        let capabilities = c.capability_overrides();
        ConfigResponse {
            id: c.id,
            name: c.name,
            provider_type: c.provider_type,
            base_url: c.base_url,
            api_key_masked,
            token: c.token,
            capabilities,
            models: models.into_iter().map(Into::into).collect(),
            created_at: c.created_at,
        }
    }
}

impl ProviderConfig {
    pub fn capability_overrides(&self) -> ProviderCapabilityOverrides {
        self.capabilities_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default()
    }
}

fn mask_key(key: &str) -> String {
    let len = key.len();
    if len <= 8 {
        return "*".repeat(len);
    }
    format!("{}...{}", &key[..4], &key[len - 4..])
}

#[cfg(test)]
mod tests {
    use super::ProviderConfig;

    #[test]
    fn subscription_anthropic_variants_use_anthropic_auth_headers() {
        for provider_type in [
            "kimi_coding_anthropic",
            "zai_coding_anthropic",
            "zhipu_coding",
            "minimax_token",
            "bailian_coding_anthropic",
            "bailian_token_anthropic",
        ] {
            assert!(provider(provider_type).uses_anthropic_auth());
        }
    }

    #[test]
    fn api_anthropic_variants_use_anthropic_auth_headers() {
        for provider_type in [
            "deepseek_anthropic",
            "qwen_anthropic",
            "zhipu_anthropic",
            "minimax_anthropic",
        ] {
            assert!(provider(provider_type).uses_anthropic_auth());
        }
    }

    #[test]
    fn subscription_openai_variants_keep_bearer_auth() {
        for provider_type in [
            "kimi_coding",
            "zai_coding_openai",
            "zhipu_coding_openai",
            "minimax_token_openai",
            "bailian_coding_openai",
            "bailian_token_openai",
        ] {
            assert!(!provider(provider_type).uses_anthropic_auth());
        }
    }

    fn provider(provider_type: &str) -> ProviderConfig {
        ProviderConfig {
            id: "provider-1".to_string(),
            name: "Provider".to_string(),
            provider_type: provider_type.to_string(),
            base_url: "https://example.test".to_string(),
            api_key: "sk-test".to_string(),
            token: "token".to_string(),
            capabilities_json: None,
            created_at: "2026-06-05T00:00:00Z".to_string(),
        }
    }
}
