mod capabilities;
mod urls;

pub use capabilities::{
    resolved_upstream_protocols, AuthScheme, ProviderCapabilities, ProviderSpec, UpstreamProtocol,
};
pub use urls::{normalize_upstream_base_url, provider_upstream_base_url};

#[cfg(test)]
mod tests {
    use super::{
        provider_upstream_base_url, resolved_upstream_protocols, AuthScheme, ProviderSpec,
        UpstreamProtocol,
    };
    use crate::models::{ProviderCapabilityOverrides, ProviderConfig};

    #[test]
    fn maps_openai_to_native_responses_with_bearer_auth_and_tools() {
        let spec = ProviderSpec::from_provider_config(None, &provider("openai"));

        assert_eq!(spec.protocol, UpstreamProtocol::Responses);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert!(spec.capabilities.tool_calls);
        assert!(spec.capabilities.reasoning);
        assert!(spec.capabilities.structured_outputs);
        assert!(spec.capabilities.streaming_usage);
    }

    #[test]
    fn maps_custom_responses_compatible_to_native_responses() {
        let spec = ProviderSpec::from_provider_config(None, &provider("responses_compatible"));

        assert_eq!(spec.protocol, UpstreamProtocol::Responses);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert!(spec.capabilities.tool_calls);
        assert!(spec.capabilities.reasoning);
    }

    #[test]
    fn maps_gotoken_to_all_documented_upstream_protocols() {
        let spec = ProviderSpec::from_provider_config(None, &provider("gotoken"));

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
        let spec = ProviderSpec::from_provider_config(None, &provider("anthropic_compatible"));

        assert_eq!(spec.protocol, UpstreamProtocol::AnthropicMessages);
        assert_eq!(spec.auth_scheme, AuthScheme::Anthropic);
        assert!(spec.capabilities.tool_calls);
    }

    #[test]
    fn preserves_unknown_provider_type_as_chat_completions_bearer_without_tools() {
        let spec = ProviderSpec::from_provider_config(None, &provider("custom"));

        assert_eq!(spec.protocol, UpstreamProtocol::ChatCompletions);
        assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
        assert!(!spec.capabilities.tool_calls);
        assert!(spec.capabilities.system_messages);
    }

    #[test]
    fn preserves_legacy_openai_compatible_variants_without_tool_capability() {
        for provider_type in ["minimax", "zhipu"] {
            let spec = ProviderSpec::from_provider_config(None, &provider(provider_type));

            assert_eq!(spec.protocol, UpstreamProtocol::ChatCompletions);
            assert_eq!(spec.auth_scheme, AuthScheme::Bearer);
            assert!(!spec.capabilities.tool_calls);
        }
    }

    #[test]
    fn maps_new_api_service_variants_to_chat_completions_with_tools() {
        for provider_type in ["kimi", "deepseek", "qwen"] {
            let spec = ProviderSpec::from_provider_config(None, &provider(provider_type));

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
            let spec = ProviderSpec::from_provider_config(None, &provider(provider_type));

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
            let spec = ProviderSpec::from_provider_config(None, &provider(provider_type));

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
            let spec = ProviderSpec::from_provider_config(None, &provider(provider_type));

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
            let spec = ProviderSpec::from_provider_config(None, &provider(provider_type));

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

        let spec = ProviderSpec::from_provider_config(None, &provider);

        assert!(spec.capabilities.tool_calls);
        assert!(spec.capabilities.structured_outputs);
        assert_eq!(spec.capabilities.max_context_tokens, Some(8192));
        assert_eq!(spec.capabilities.max_output_tokens, Some(2048));
    }

    #[test]
    fn catalog_protocols_win_over_saved_protocol_overrides() {
        let catalog = crate::test_support::fixture_catalog();
        let mut provider = provider("deepseek");
        provider.capabilities_json = Some(
            serde_json::to_string(&ProviderCapabilityOverrides {
                upstream_protocols: Some(vec!["images_generations".to_string()]),
                ..ProviderCapabilityOverrides::default()
            })
            .expect("encode capabilities"),
        );

        let spec = ProviderSpec::from_provider_config(Some(&catalog), &provider);

        assert_eq!(
            spec.supported_protocols,
            vec![
                UpstreamProtocol::ChatCompletions,
                UpstreamProtocol::AnthropicMessages,
            ]
        );
        assert_eq!(spec.upstream_for_downstream("images_generations"), None);
    }

    #[test]
    fn enables_image_generations_only_from_catalog_declaration() {
        let catalog = crate::test_support::fixture_catalog();

        let image_only =
            ProviderSpec::from_provider_config(Some(&catalog), &provider("image_only"));
        assert_eq!(
            image_only.upstream_for_downstream("images_generations"),
            Some(UpstreamProtocol::ImageGenerations)
        );

        let deepseek = ProviderSpec::from_provider_config(Some(&catalog), &provider("deepseek"));
        assert_eq!(deepseek.upstream_for_downstream("images_generations"), None);
    }

    #[test]
    fn resolves_protocol_values_from_catalog_or_legacy_provider_type() {
        let catalog = crate::test_support::fixture_catalog();

        assert_eq!(
            resolved_upstream_protocols(Some(&catalog), "deepseek"),
            ["openai", "anthropic"]
        );
        assert_eq!(
            resolved_upstream_protocols(Some(&catalog), "gotoken"),
            ["openai", "responses", "anthropic", "images_generations"]
        );
        assert_eq!(
            resolved_upstream_protocols(None, "kimi_coding_anthropic"),
            ["openai", "anthropic"]
        );
    }

    #[test]
    fn built_in_provider_capabilities_are_not_limited_by_saved_protocol_variant() {
        let spec = ProviderSpec::from_provider_config(None, &provider("kimi_coding_anthropic"));

        assert_eq!(spec.protocol, UpstreamProtocol::AnthropicMessages);
        assert!(spec.supports_downstream("responses"));
        assert!(spec.supports_downstream("chat_completions"));
        assert!(spec.supports_downstream("anthropic_messages"));
    }

    #[test]
    fn chooses_upstream_protocol_by_downstream_request_preference() {
        let spec = ProviderSpec::from_provider_config(None, &provider("kimi_coding_anthropic"));

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

        let spec = ProviderSpec::from_provider_config(None, &provider("anthropic_compatible"));
        assert_eq!(
            spec.upstream_for_downstream("responses"),
            Some(UpstreamProtocol::AnthropicMessages)
        );
        assert_eq!(spec.upstream_for_downstream("chat_completions"), None);
    }

    #[test]
    fn resolves_protocol_base_url_from_saved_settings_then_base_url() {
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
    fn falls_back_to_base_url_without_protocol_settings() {
        let provider = provider_with_protocol_base_urls(
            "https://default.example/v1",
            None,
            Some(" "),
            None,
            Some(" "),
        );

        for protocol in [
            UpstreamProtocol::Responses,
            UpstreamProtocol::ChatCompletions,
            UpstreamProtocol::AnthropicMessages,
            UpstreamProtocol::ImageGenerations,
        ] {
            assert_eq!(
                provider_upstream_base_url(&provider, protocol),
                "https://default.example/v1"
            );
        }
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
            models: Vec::new(),
            created_at: "2026-06-05T00:00:00Z".to_string(),
        }
    }
}
