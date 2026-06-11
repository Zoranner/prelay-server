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
    pub reasoning: bool,
    pub tool_choice: bool,
    pub parallel_tool_calls: bool,
    pub system_messages: bool,
    pub structured_outputs: bool,
    pub streaming_usage: bool,
    pub max_context_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
}

impl ProviderCapabilities {
    pub fn limited() -> Self {
        Self {
            tool_calls: false,
            reasoning: false,
            tool_choice: false,
            parallel_tool_calls: false,
            system_messages: true,
            structured_outputs: false,
            streaming_usage: false,
            max_context_tokens: None,
            max_output_tokens: None,
        }
    }

    fn with_overrides(self, provider: &ProviderConfig) -> Self {
        let overrides = provider.capability_overrides();
        Self {
            tool_calls: overrides.tool_calls.unwrap_or(self.tool_calls),
            reasoning: overrides.reasoning.unwrap_or(self.reasoning),
            tool_choice: overrides.tool_choice.unwrap_or(self.tool_choice),
            parallel_tool_calls: overrides
                .parallel_tool_calls
                .unwrap_or(self.parallel_tool_calls),
            system_messages: overrides.system_messages.unwrap_or(self.system_messages),
            structured_outputs: overrides
                .structured_outputs
                .unwrap_or(self.structured_outputs),
            streaming_usage: overrides.streaming_usage.unwrap_or(self.streaming_usage),
            max_context_tokens: overrides.max_context_tokens.or(self.max_context_tokens),
            max_output_tokens: overrides.max_output_tokens.or(self.max_output_tokens),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderSpec {
    pub protocol: UpstreamProtocol,
    pub auth_scheme: AuthScheme,
    pub capabilities: ProviderCapabilities,
}

impl ProviderSpec {
    pub fn from_provider_config(provider: &ProviderConfig) -> Self {
        let mut spec = match provider.provider_type.as_str() {
            "openai" => Self {
                protocol: UpstreamProtocol::Responses,
                auth_scheme: AuthScheme::Bearer,
                capabilities: ProviderCapabilities {
                    tool_calls: true,
                    reasoning: true,
                    tool_choice: true,
                    parallel_tool_calls: true,
                    system_messages: true,
                    structured_outputs: true,
                    streaming_usage: true,
                    max_context_tokens: None,
                    max_output_tokens: None,
                },
            },
            "openai_compatible" => Self {
                protocol: UpstreamProtocol::ChatCompletions,
                auth_scheme: AuthScheme::Bearer,
                capabilities: ProviderCapabilities {
                    tool_calls: true,
                    reasoning: false,
                    tool_choice: true,
                    parallel_tool_calls: true,
                    system_messages: true,
                    structured_outputs: false,
                    streaming_usage: false,
                    max_context_tokens: None,
                    max_output_tokens: None,
                },
            },
            "anthropic" | "anthropic_compatible" => Self {
                protocol: UpstreamProtocol::AnthropicMessages,
                auth_scheme: AuthScheme::Anthropic,
                capabilities: ProviderCapabilities {
                    tool_calls: true,
                    reasoning: false,
                    tool_choice: true,
                    parallel_tool_calls: false,
                    system_messages: true,
                    structured_outputs: false,
                    streaming_usage: true,
                    max_context_tokens: None,
                    max_output_tokens: None,
                },
            },
            "minimax_token" | "zhipu_coding" => Self {
                protocol: UpstreamProtocol::AnthropicMessages,
                auth_scheme: AuthScheme::Anthropic,
                capabilities: ProviderCapabilities::limited(),
            },
            "ollama_native" => Self {
                protocol: UpstreamProtocol::OllamaNative,
                auth_scheme: AuthScheme::None,
                capabilities: ProviderCapabilities {
                    tool_calls: false,
                    reasoning: false,
                    tool_choice: false,
                    parallel_tool_calls: false,
                    system_messages: true,
                    structured_outputs: false,
                    streaming_usage: false,
                    max_context_tokens: None,
                    max_output_tokens: None,
                },
            },
            _ => Self {
                protocol: UpstreamProtocol::ChatCompletions,
                auth_scheme: AuthScheme::Bearer,
                capabilities: ProviderCapabilities::limited(),
            },
        };
        spec.capabilities = spec.capabilities.with_overrides(provider);
        spec
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthScheme, ProviderSpec, UpstreamProtocol};
    use crate::models::{ProviderCapabilityOverrides, ProviderConfig};

    #[test]
    fn maps_openai_to_native_responses_with_bearer_auth_and_tools() {
        let spec = ProviderSpec::from_provider_config(&provider("openai"));

        assert_eq!(spec.protocol, UpstreamProtocol::Responses);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert!(spec.capabilities.tool_calls);
        assert!(spec.capabilities.reasoning);
        assert!(spec.capabilities.structured_outputs);
        assert!(spec.capabilities.streaming_usage);
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
        assert!(spec.capabilities.system_messages);
        assert!(!spec.capabilities.structured_outputs);
    }

    #[test]
    fn preserves_unknown_provider_type_as_chat_completions_bearer_without_tools() {
        let spec = ProviderSpec::from_provider_config(&provider("custom"));

        assert_eq!(spec.protocol, UpstreamProtocol::ChatCompletions);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert!(!spec.capabilities.tool_calls);
        assert!(spec.capabilities.system_messages);
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

    #[test]
    fn applies_provider_capability_overrides() {
        let mut provider = provider("ollama_native");
        provider.capabilities_json = Some(
            serde_json::to_string(&ProviderCapabilityOverrides {
                tool_calls: Some(true),
                structured_outputs: Some(true),
                max_context_tokens: Some(8192),
                max_output_tokens: Some(2048),
                ..ProviderCapabilityOverrides::default()
            })
            .expect("encode capabilities"),
        );

        let spec = ProviderSpec::from_provider_config(&provider);

        assert!(spec.capabilities.tool_calls);
        assert!(spec.capabilities.structured_outputs);
        assert_eq!(spec.capabilities.max_context_tokens, Some(8192));
        assert_eq!(spec.capabilities.max_output_tokens, Some(2048));
    }

    fn provider(provider_type: &str) -> ProviderConfig {
        ProviderConfig {
            id: "provider-1".to_string(),
            name: "Provider".to_string(),
            provider_type: provider_type.to_string(),
            base_url: "http://127.0.0.1:11434/api".to_string(),
            api_key: "sk-test".to_string(),
            token: "token".to_string(),
            capabilities_json: None,
            created_at: "2026-06-05T00:00:00Z".to_string(),
        }
    }
}
