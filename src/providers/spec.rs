use crate::models::{ProviderCapabilityOverrides, ProviderConfig};
use prelay_protocol::ProviderResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamProtocol {
    Responses,
    ChatCompletions,
    AnthropicMessages,
    ImageGenerations,
}

impl UpstreamProtocol {
    pub fn downstream_protocols(self) -> &'static [&'static str] {
        match self {
            UpstreamProtocol::Responses => &["responses", "anthropic_messages"],
            UpstreamProtocol::ChatCompletions => {
                &["responses", "chat_completions", "anthropic_messages"]
            }
            UpstreamProtocol::AnthropicMessages => &["responses", "anthropic_messages"],
            UpstreamProtocol::ImageGenerations => &["images_generations"],
        }
    }

    pub fn supports_downstream(self, downstream_protocol: &str) -> bool {
        self.downstream_protocols().contains(&downstream_protocol)
    }

    pub(crate) fn from_capability_value(value: &str) -> Option<Self> {
        match value {
            "responses" => Some(UpstreamProtocol::Responses),
            "openai" => Some(UpstreamProtocol::ChatCompletions),
            "anthropic" => Some(UpstreamProtocol::AnthropicMessages),
            "images_generations" => Some(UpstreamProtocol::ImageGenerations),
            _ => None,
        }
    }

    fn capability_value(self) -> &'static str {
        match self {
            UpstreamProtocol::Responses => "responses",
            UpstreamProtocol::ChatCompletions => "openai",
            UpstreamProtocol::AnthropicMessages => "anthropic",
            UpstreamProtocol::ImageGenerations => "images_generations",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    Bearer,
    Anthropic,
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

    fn with_overrides(self, overrides: &ProviderCapabilityOverrides) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    pub protocol: UpstreamProtocol,
    pub supported_protocols: Vec<UpstreamProtocol>,
    pub auth_scheme: AuthScheme,
    pub capabilities: ProviderCapabilities,
}

impl ProviderSpec {
    pub fn from_provider_config(provider: &ProviderConfig) -> Self {
        Self::from_provider_type_and_overrides(
            &provider.provider_type,
            &provider.capability_overrides(),
        )
    }

    fn from_provider_type_and_overrides(
        provider_type: &str,
        overrides: &ProviderCapabilityOverrides,
    ) -> Self {
        let mut spec = match provider_type {
            "openai"
            | "responses_compatible"
            | "qwen_responses"
            | "minimax_responses"
            | "gotoken" => Self {
                protocol: UpstreamProtocol::Responses,
                supported_protocols: provider_supported_protocols(
                    provider_type,
                    UpstreamProtocol::Responses,
                ),
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
            "openai_compatible"
            | "kimi"
            | "deepseek"
            | "qwen"
            | "kimi_coding"
            | "zai_coding_openai"
            | "zhipu_coding_openai"
            | "minimax_token_openai"
            | "bailian_coding_openai"
            | "bailian_token_openai" => Self {
                protocol: UpstreamProtocol::ChatCompletions,
                supported_protocols: provider_supported_protocols(
                    provider_type,
                    UpstreamProtocol::ChatCompletions,
                ),
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
            | "bailian_token_anthropic" => Self {
                protocol: UpstreamProtocol::AnthropicMessages,
                supported_protocols: provider_supported_protocols(
                    provider_type,
                    UpstreamProtocol::AnthropicMessages,
                ),
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
            _ => Self {
                protocol: UpstreamProtocol::ChatCompletions,
                supported_protocols: vec![UpstreamProtocol::ChatCompletions],
                auth_scheme: AuthScheme::Bearer,
                capabilities: ProviderCapabilities::limited(),
            },
        };
        spec.capabilities = spec.capabilities.with_overrides(overrides);
        if let Some(protocols) = supported_protocols_from_overrides(overrides) {
            spec.supported_protocols = protocols;
        }
        spec
    }

    pub fn supports_downstream(&self, downstream_protocol: &str) -> bool {
        self.supported_protocols
            .iter()
            .any(|protocol| protocol.supports_downstream(downstream_protocol))
    }

    pub fn upstream_for_downstream(&self, downstream_protocol: &str) -> Option<UpstreamProtocol> {
        let preference = match downstream_protocol {
            "responses" => &[
                UpstreamProtocol::Responses,
                UpstreamProtocol::ChatCompletions,
                UpstreamProtocol::AnthropicMessages,
            ][..],
            "chat_completions" => &[UpstreamProtocol::ChatCompletions][..],
            "anthropic_messages" => &[
                UpstreamProtocol::AnthropicMessages,
                UpstreamProtocol::ChatCompletions,
                UpstreamProtocol::Responses,
            ][..],
            "images_generations" => &[UpstreamProtocol::ImageGenerations][..],
            _ => &[][..],
        };
        preference
            .iter()
            .copied()
            .find(|protocol| self.supported_protocols.contains(protocol))
    }
}

pub fn resolved_upstream_protocols(
    provider_type: &str,
    configured_protocols: Option<&[String]>,
) -> Vec<String> {
    let overrides = ProviderCapabilityOverrides {
        upstream_protocols: configured_protocols.map(<[String]>::to_vec),
        ..ProviderCapabilityOverrides::default()
    };
    ProviderSpec::from_provider_type_and_overrides(provider_type, &overrides)
        .supported_protocols
        .into_iter()
        .map(UpstreamProtocol::capability_value)
        .map(str::to_owned)
        .collect()
}

pub fn provider_upstream_base_url(
    provider: &ProviderConfig,
    upstream_protocol: UpstreamProtocol,
) -> String {
    let overrides = provider.capability_overrides();
    let protocol_base_url =
        overrides
            .protocol_base_urls
            .as_ref()
            .and_then(|base_urls| match upstream_protocol {
                UpstreamProtocol::Responses => base_urls.responses.as_deref(),
                UpstreamProtocol::ChatCompletions => base_urls.openai.as_deref(),
                UpstreamProtocol::AnthropicMessages => base_urls.anthropic.as_deref(),
                UpstreamProtocol::ImageGenerations => base_urls.images_generations.as_deref(),
            });

    resolve_provider_upstream_base_url(
        &provider.provider_type,
        &provider.base_url,
        protocol_base_url,
        upstream_protocol,
    )
}

pub fn provider_response_upstream_base_url(
    provider: &ProviderResponse,
    upstream_protocol: UpstreamProtocol,
) -> String {
    let protocol_base_url =
        provider
            .capabilities
            .protocol_base_urls
            .as_ref()
            .and_then(|base_urls| match upstream_protocol {
                UpstreamProtocol::Responses => base_urls.responses.as_deref(),
                UpstreamProtocol::ChatCompletions => base_urls.openai.as_deref(),
                UpstreamProtocol::AnthropicMessages => base_urls.anthropic.as_deref(),
                UpstreamProtocol::ImageGenerations => base_urls.images_generations.as_deref(),
            });

    resolve_provider_upstream_base_url(
        &provider.provider_type,
        &provider.base_url,
        protocol_base_url,
        upstream_protocol,
    )
}

fn resolve_provider_upstream_base_url(
    provider_type: &str,
    base_url: &str,
    protocol_base_url: Option<&str>,
    upstream_protocol: UpstreamProtocol,
) -> String {
    let protocol_base_url = protocol_base_url
        .map(str::trim)
        .filter(|base_url| !base_url.is_empty());
    normalize_upstream_base_url(
        provider_type,
        upstream_protocol,
        protocol_base_url.unwrap_or(base_url.trim()),
    )
}

pub fn normalize_upstream_base_url(
    provider_type: &str,
    upstream_protocol: UpstreamProtocol,
    base_url: &str,
) -> String {
    let base_url = base_url.trim().trim_end_matches('/');
    if matches!(provider_type, "kimi_coding" | "kimi_coding_anthropic")
        && upstream_protocol == UpstreamProtocol::AnthropicMessages
        && base_url == "https://api.kimi.com/coding"
    {
        return format!("{base_url}/v1");
    }
    if provider_type == "gotoken"
        && upstream_protocol == UpstreamProtocol::AnthropicMessages
        && base_url == "https://gotoken.cc"
    {
        return format!("{base_url}/v1");
    }
    base_url.to_string()
}

fn supported_protocols_from_overrides(
    overrides: &ProviderCapabilityOverrides,
) -> Option<Vec<UpstreamProtocol>> {
    let values = overrides.upstream_protocols.as_ref()?;
    let mut protocols = Vec::new();
    for value in values {
        let Some(protocol) = UpstreamProtocol::from_capability_value(value) else {
            continue;
        };
        if !protocols.contains(&protocol) {
            protocols.push(protocol);
        }
    }
    (!protocols.is_empty()).then_some(protocols)
}

fn provider_supported_protocols(
    provider_type: &str,
    default_protocol: UpstreamProtocol,
) -> Vec<UpstreamProtocol> {
    match provider_type {
        "kimi_coding"
        | "kimi_coding_anthropic"
        | "zhipu_coding_openai"
        | "zhipu_coding"
        | "minimax_token_openai"
        | "minimax_token"
        | "deepseek"
        | "deepseek_anthropic"
        | "zhipu"
        | "zhipu_anthropic" => {
            vec![
                UpstreamProtocol::ChatCompletions,
                UpstreamProtocol::AnthropicMessages,
            ]
        }
        "qwen" | "qwen_responses" | "qwen_anthropic" | "minimax" | "minimax_responses"
        | "minimax_anthropic" | "gotoken" => vec![
            UpstreamProtocol::Responses,
            UpstreamProtocol::ChatCompletions,
            UpstreamProtocol::AnthropicMessages,
        ],
        _ => vec![default_protocol],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        provider_upstream_base_url, resolved_upstream_protocols, AuthScheme, ProviderSpec,
        UpstreamProtocol,
    };
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
    fn maps_custom_responses_compatible_to_native_responses() {
        let spec = ProviderSpec::from_provider_config(&provider("responses_compatible"));

        assert_eq!(spec.protocol, UpstreamProtocol::Responses);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert!(spec.capabilities.tool_calls);
        assert!(spec.capabilities.reasoning);
    }

    #[test]
    fn maps_gotoken_to_all_documented_upstream_protocols() {
        let spec = ProviderSpec::from_provider_config(&provider("gotoken"));

        assert_eq!(spec.protocol, UpstreamProtocol::Responses);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert_eq!(
            spec.supported_protocols,
            vec![
                UpstreamProtocol::Responses,
                UpstreamProtocol::ChatCompletions,
                UpstreamProtocol::AnthropicMessages,
            ]
        );
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
    fn maps_new_api_service_variants_to_chat_completions_with_tools() {
        for provider_type in ["kimi", "deepseek", "qwen"] {
            let spec = ProviderSpec::from_provider_config(&provider(provider_type));

            assert_eq!(spec.protocol, UpstreamProtocol::ChatCompletions);
            assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
            assert!(spec.capabilities.tool_calls);
            assert!(spec.capabilities.tool_choice);
            assert!(spec.capabilities.system_messages);
        }
    }

    #[test]
    fn maps_api_service_protocol_variants_to_documented_upstream_protocols() {
        for provider_type in ["qwen_responses", "minimax_responses"] {
            let spec = ProviderSpec::from_provider_config(&provider(provider_type));

            assert_eq!(spec.protocol, UpstreamProtocol::Responses);
            assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
            assert!(spec.capabilities.tool_calls);
        }

        for provider_type in [
            "deepseek_anthropic",
            "qwen_anthropic",
            "zhipu_anthropic",
            "minimax_anthropic",
        ] {
            let spec = ProviderSpec::from_provider_config(&provider(provider_type));

            assert_eq!(spec.protocol, UpstreamProtocol::AnthropicMessages);
            assert_eq!(spec.auth_scheme, AuthScheme::Anthropic);
            assert!(spec.capabilities.tool_calls);
        }
    }

    #[test]
    fn maps_subscription_anthropic_variants_to_messages_with_tools() {
        for provider_type in [
            "kimi_coding_anthropic",
            "zai_coding_anthropic",
            "zhipu_coding",
            "minimax_token",
            "bailian_coding_anthropic",
            "bailian_token_anthropic",
        ] {
            let spec = ProviderSpec::from_provider_config(&provider(provider_type));

            assert_eq!(spec.protocol, UpstreamProtocol::AnthropicMessages);
            assert_eq!(spec.auth_scheme, AuthScheme::Anthropic);
            assert!(spec.capabilities.tool_calls);
            assert!(spec.capabilities.tool_choice);
        }
    }

    #[test]
    fn maps_subscription_openai_variants_to_chat_completions_with_tools() {
        for provider_type in [
            "kimi_coding",
            "zai_coding_openai",
            "zhipu_coding_openai",
            "minimax_token_openai",
            "bailian_coding_openai",
            "bailian_token_openai",
        ] {
            let spec = ProviderSpec::from_provider_config(&provider(provider_type));

            assert_eq!(spec.protocol, UpstreamProtocol::ChatCompletions);
            assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
            assert!(spec.capabilities.tool_calls);
            assert!(spec.capabilities.tool_choice);
            assert!(spec.capabilities.system_messages);
        }
    }

    #[test]
    fn applies_provider_capability_overrides() {
        let mut provider = provider("openai_compatible");
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

    #[test]
    fn custom_provider_protocol_declarations_override_provider_type_defaults() {
        let mut provider = provider("openai_compatible");
        provider.capabilities_json = Some(
            serde_json::to_string(&ProviderCapabilityOverrides {
                upstream_protocols: Some(vec!["anthropic".to_string()]),
                ..ProviderCapabilityOverrides::default()
            })
            .expect("encode capabilities"),
        );

        let spec = ProviderSpec::from_provider_config(&provider);

        assert!(spec.supports_downstream("responses"));
        assert!(spec.supports_downstream("anthropic_messages"));
        assert!(!spec.supports_downstream("chat_completions"));
    }

    #[test]
    fn enables_image_generations_only_when_explicitly_declared() {
        let mut configured_provider = provider("gotoken");
        configured_provider.capabilities_json = Some(
            serde_json::to_string(&ProviderCapabilityOverrides {
                upstream_protocols: Some(vec!["images_generations".to_string()]),
                ..ProviderCapabilityOverrides::default()
            })
            .expect("encode capabilities"),
        );

        let explicitly_declared = ProviderSpec::from_provider_config(&configured_provider);
        assert_eq!(
            explicitly_declared.upstream_for_downstream("images_generations"),
            Some(UpstreamProtocol::ImageGenerations)
        );
        assert_eq!(
            ProviderSpec::from_provider_config(&provider("gotoken"))
                .upstream_for_downstream("images_generations"),
            None
        );
        assert_eq!(
            ProviderSpec::from_provider_config(&provider("openai"))
                .upstream_for_downstream("images_generations"),
            None
        );
    }

    #[test]
    fn resolves_protocol_values_from_provider_type_and_capability_overrides() {
        let default_protocols = resolved_upstream_protocols("kimi_coding_anthropic", None);
        assert_eq!(default_protocols, ["openai", "anthropic"]);

        let overridden_protocols = resolved_upstream_protocols(
            "openai_compatible",
            Some(&["anthropic".to_string(), "unknown".to_string()]),
        );
        assert_eq!(overridden_protocols, ["anthropic"]);
    }

    #[test]
    fn built_in_provider_capabilities_are_not_limited_by_saved_protocol_variant() {
        let spec = ProviderSpec::from_provider_config(&provider("kimi_coding_anthropic"));

        assert_eq!(spec.protocol, UpstreamProtocol::AnthropicMessages);
        assert!(spec.supports_downstream("responses"));
        assert!(spec.supports_downstream("chat_completions"));
        assert!(spec.supports_downstream("anthropic_messages"));
    }

    #[test]
    fn chooses_upstream_protocol_by_downstream_request_preference() {
        let spec = ProviderSpec::from_provider_config(&provider("kimi_coding_anthropic"));

        assert_eq!(
            spec.upstream_for_downstream("responses"),
            Some(UpstreamProtocol::ChatCompletions)
        );
        assert_eq!(
            spec.upstream_for_downstream("chat_completions"),
            Some(UpstreamProtocol::ChatCompletions)
        );
        assert_eq!(
            spec.upstream_for_downstream("anthropic_messages"),
            Some(UpstreamProtocol::AnthropicMessages)
        );

        let spec = ProviderSpec::from_provider_config(&provider("anthropic_compatible"));
        assert_eq!(
            spec.upstream_for_downstream("responses"),
            Some(UpstreamProtocol::AnthropicMessages)
        );
        assert_eq!(spec.upstream_for_downstream("chat_completions"), None);
    }

    #[test]
    fn resolves_protocol_base_url_from_capability_overrides() {
        let provider = provider_with_protocol_base_urls(
            "https://default.example/v1",
            Some("https://responses.example/v1"),
            Some("https://chat.example/v1"),
            Some("https://anthropic.example"),
            Some("https://images.example/v1"),
        );

        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::Responses),
            "https://responses.example/v1"
        );
        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::ChatCompletions),
            "https://chat.example/v1"
        );
        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::AnthropicMessages),
            "https://anthropic.example"
        );
        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::ImageGenerations),
            "https://images.example/v1"
        );
    }

    #[test]
    fn falls_back_to_default_base_url_when_protocol_base_url_is_empty() {
        let provider = provider_with_protocol_base_urls(
            "https://default.example/v1",
            None,
            Some(" "),
            None,
            Some(" "),
        );

        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::Responses),
            "https://default.example/v1"
        );
        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::ChatCompletions),
            "https://default.example/v1"
        );
        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::AnthropicMessages),
            "https://default.example/v1"
        );
        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::ImageGenerations),
            "https://default.example/v1"
        );
    }

    #[test]
    fn normalizes_kimi_code_anthropic_base_url_to_versioned_messages_root() {
        let mut provider = provider("kimi_coding_anthropic");
        provider.base_url = "https://api.kimi.com/coding/".to_string();

        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::AnthropicMessages),
            "https://api.kimi.com/coding/v1"
        );
    }

    #[test]
    fn normalizes_gotoken_anthropic_root_to_versioned_messages_root() {
        let mut provider = provider("gotoken");
        provider.base_url = "https://gotoken.cc".to_string();

        assert_eq!(
            provider_upstream_base_url(&provider, UpstreamProtocol::AnthropicMessages),
            "https://gotoken.cc/v1"
        );
    }

    #[test]
    fn enforces_downstream_protocol_matrix() {
        assert!(UpstreamProtocol::Responses.supports_downstream("responses"));
        assert!(UpstreamProtocol::Responses.supports_downstream("anthropic_messages"));
        assert!(!UpstreamProtocol::Responses.supports_downstream("chat_completions"));

        assert!(UpstreamProtocol::AnthropicMessages.supports_downstream("responses"));
        assert!(!UpstreamProtocol::AnthropicMessages.supports_downstream("chat_completions"));

        assert!(UpstreamProtocol::ChatCompletions.supports_downstream("responses"));
        assert!(UpstreamProtocol::ChatCompletions.supports_downstream("chat_completions"));
        assert!(UpstreamProtocol::ChatCompletions.supports_downstream("anthropic_messages"));

        assert!(UpstreamProtocol::ImageGenerations.supports_downstream("images_generations"));
        assert!(!UpstreamProtocol::ImageGenerations.supports_downstream("responses"));
    }

    fn provider_with_protocol_base_urls(
        base_url: &str,
        responses: Option<&str>,
        openai: Option<&str>,
        anthropic: Option<&str>,
        images_generations: Option<&str>,
    ) -> ProviderConfig {
        let mut provider = provider("openai_compatible");
        provider.base_url = base_url.to_string();
        provider.capabilities_json = Some(
            serde_json::to_string(&ProviderCapabilityOverrides {
                protocol_base_urls: Some(crate::models::ProviderProtocolBaseUrls {
                    responses: responses.map(str::to_string),
                    openai: openai.map(str::to_string),
                    anthropic: anthropic.map(str::to_string),
                    images_generations: images_generations.map(str::to_string),
                }),
                ..ProviderCapabilityOverrides::default()
            })
            .expect("encode capabilities"),
        );
        provider
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
