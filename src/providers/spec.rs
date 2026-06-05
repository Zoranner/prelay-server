use crate::models::ProviderConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamProtocol {
    Responses,
    ChatCompletions,
    AnthropicMessages,
    OllamaNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    Bearer,
    Anthropic,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub tool_calls: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderSpec {
    pub protocol: UpstreamProtocol,
    pub auth_scheme: AuthScheme,
    pub capabilities: ProviderCapabilities,
}

impl ProviderSpec {
    pub fn from_provider_config(provider: &ProviderConfig) -> Self {
        match provider.provider_type.as_str() {
            "openai" => Self {
                protocol: UpstreamProtocol::Responses,
                auth_scheme: AuthScheme::Bearer,
                capabilities: ProviderCapabilities { tool_calls: true },
            },
            "openai_compatible" => Self {
                protocol: UpstreamProtocol::ChatCompletions,
                auth_scheme: AuthScheme::Bearer,
                capabilities: ProviderCapabilities { tool_calls: true },
            },
            "anthropic" | "anthropic_compatible" => Self {
                protocol: UpstreamProtocol::AnthropicMessages,
                auth_scheme: AuthScheme::Anthropic,
                capabilities: ProviderCapabilities { tool_calls: true },
            },
            "minimax_token" | "zhipu_coding" => Self {
                protocol: UpstreamProtocol::AnthropicMessages,
                auth_scheme: AuthScheme::Anthropic,
                capabilities: ProviderCapabilities { tool_calls: false },
            },
            "ollama_native" => Self {
                protocol: UpstreamProtocol::OllamaNative,
                auth_scheme: AuthScheme::None,
                capabilities: ProviderCapabilities { tool_calls: false },
            },
            _ => Self {
                protocol: UpstreamProtocol::ChatCompletions,
                auth_scheme: AuthScheme::Bearer,
                capabilities: ProviderCapabilities { tool_calls: false },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthScheme, ProviderSpec, UpstreamProtocol};
    use crate::models::ProviderConfig;

    #[test]
    fn maps_openai_to_native_responses_with_bearer_auth_and_tools() {
        let spec = ProviderSpec::from_provider_config(&provider("openai"));

        assert_eq!(spec.protocol, UpstreamProtocol::Responses);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert!(spec.capabilities.tool_calls);
    }

    #[test]
    fn maps_anthropic_compatible_to_messages_with_anthropic_auth_and_tools() {
        let spec = ProviderSpec::from_provider_config(&provider("anthropic_compatible"));

        assert_eq!(spec.protocol, UpstreamProtocol::AnthropicMessages);
        assert_eq!(spec.auth_scheme, AuthScheme::Anthropic);
        assert!(spec.capabilities.tool_calls);
    }

    #[test]
    fn maps_ollama_native_to_native_chat_without_auth_or_tools() {
        let spec = ProviderSpec::from_provider_config(&provider("ollama_native"));

        assert_eq!(spec.protocol, UpstreamProtocol::OllamaNative);
        assert_eq!(spec.auth_scheme, AuthScheme::None);
        assert!(!spec.capabilities.tool_calls);
    }

    #[test]
    fn preserves_unknown_provider_type_as_chat_completions_bearer_without_tools() {
        let spec = ProviderSpec::from_provider_config(&provider("custom"));

        assert_eq!(spec.protocol, UpstreamProtocol::ChatCompletions);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert!(!spec.capabilities.tool_calls);
    }

    #[test]
    fn preserves_legacy_openai_compatible_variants_without_tool_capability() {
        for provider_type in ["minimax", "zhipu"] {
            let spec = ProviderSpec::from_provider_config(&provider(provider_type));

            assert_eq!(spec.protocol, UpstreamProtocol::ChatCompletions);
            assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
            assert!(!spec.capabilities.tool_calls);
        }
    }

    #[test]
    fn preserves_legacy_anthropic_auth_variants_without_tool_capability() {
        for provider_type in ["minimax_token", "zhipu_coding"] {
            let spec = ProviderSpec::from_provider_config(&provider(provider_type));

            assert_eq!(spec.protocol, UpstreamProtocol::AnthropicMessages);
            assert_eq!(spec.auth_scheme, AuthScheme::Anthropic);
            assert!(!spec.capabilities.tool_calls);
        }
    }

    fn provider(provider_type: &str) -> ProviderConfig {
        ProviderConfig {
            id: "provider-1".to_string(),
            name: "Provider".to_string(),
            provider_type: provider_type.to_string(),
            base_url: "http://127.0.0.1:11434/api".to_string(),
            api_key: "sk-test".to_string(),
            token: "token".to_string(),
            created_at: "2026-06-05T00:00:00Z".to_string(),
        }
    }
}
