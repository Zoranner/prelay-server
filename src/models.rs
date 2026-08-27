use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
pub struct EndpointModel {
    pub id: String,
    pub endpoint_id: String,
    pub model_name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub created_at: String,
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
    pub images_generations: Option<String>,
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

impl ProviderConfig {
    pub fn capability_overrides(&self) -> ProviderCapabilityOverrides {
        self.capabilities_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default()
    }
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
