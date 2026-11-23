mod capabilities;
mod request_headers;
mod urls;

pub use capabilities::{resolved_upstream_protocols, ProviderSpec, UpstreamProtocol};
pub use request_headers::{with_provider_headers, RequestHeaderContext};
pub use urls::{normalize_upstream_base_url, provider_upstream_base_url};

#[cfg(test)]
mod tests {
    use super::{
        provider_upstream_base_url, resolved_upstream_protocols, ProviderSpec, UpstreamProtocol,
    };
    use crate::domain::registry::{ProviderCapabilityOverrides, ProviderConfig};
    use crate::test_support::fixture_catalog;

    #[test]
    fn resolves_protocols_from_catalog_entry() {
        let catalog = fixture_catalog();

        let deepseek = ProviderSpec::from_provider_config(&catalog, &provider("deepseek"));
        assert_eq!(
            deepseek.supported_protocols,
            vec![UpstreamProtocol::ChatCompletions]
        );

        let relay = ProviderSpec::from_provider_config(&catalog, &provider("relay"));
        assert_eq!(
            relay.supported_protocols,
            vec![
                UpstreamProtocol::ChatCompletions,
                UpstreamProtocol::Responses,
            ]
        );
    }

    #[test]
    fn provider_missing_from_catalog_has_no_protocols() {
        let catalog = fixture_catalog();
        let spec = ProviderSpec::from_provider_config(&catalog, &provider("retired_provider"));

        assert!(spec.supported_protocols.is_empty());
        assert_eq!(spec.upstream_for_downstream("responses"), None);
        assert_eq!(spec.upstream_for_downstream("chat_completions"), None);
        assert!(!spec.supports_downstream("responses"));
    }

    #[test]
    fn enables_image_generations_only_from_catalog_declaration() {
        let catalog = crate::test_support::fixture_catalog();

        let image_only = ProviderSpec::from_provider_config(&catalog, &provider("image_only"));
        assert_eq!(
            image_only.upstream_for_downstream("images_generations"),
            Some(UpstreamProtocol::ImageGenerations)
        );

        let deepseek = ProviderSpec::from_provider_config(&catalog, &provider("deepseek"));
        assert_eq!(deepseek.upstream_for_downstream("images_generations"), None);
    }

    #[test]
    fn resolves_protocol_values_from_catalog() {
        let catalog = crate::test_support::fixture_catalog();

        assert_eq!(
            resolved_upstream_protocols(&catalog, "deepseek"),
            ["openai"]
        );
        assert_eq!(
            resolved_upstream_protocols(&catalog, "relay"),
            ["openai", "responses"]
        );
        assert!(resolved_upstream_protocols(&catalog, "retired_provider").is_empty());
    }

    #[test]
    fn chooses_upstream_protocol_by_downstream_request_preference() {
        let catalog = crate::test_support::fixture_catalog();
        let spec = ProviderSpec::from_provider_config(&catalog, &provider("relay"));

        assert_eq!(
            spec.upstream_for_downstream("responses"),
            Some(UpstreamProtocol::Responses)
        );
        assert_eq!(
            spec.upstream_for_downstream("chat_completions"),
            Some(UpstreamProtocol::ChatCompletions)
        );
        assert_eq!(spec.upstream_for_downstream("images_generations"), None);
    }

    #[test]
    fn resolves_protocol_base_url_from_saved_settings_then_base_url() {
        let provider = provider_with_protocol_base_urls(
            "https://default.example/v1",
            Some("https://responses.example/v1"),
            Some("https://chat.example/v1"),
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
            Some(" "),
        );

        for protocol in [
            UpstreamProtocol::Responses,
            UpstreamProtocol::ChatCompletions,
            UpstreamProtocol::ImageGenerations,
        ] {
            assert_eq!(
                provider_upstream_base_url(&provider, protocol),
                "https://default.example/v1"
            );
        }
    }

    #[test]
    fn enforces_downstream_protocol_matrix() {
        assert!(UpstreamProtocol::Responses.supports_downstream("responses"));
        assert!(!UpstreamProtocol::Responses.supports_downstream("chat_completions"));

        assert!(UpstreamProtocol::ChatCompletions.supports_downstream("responses"));
        assert!(UpstreamProtocol::ChatCompletions.supports_downstream("chat_completions"));

        assert!(UpstreamProtocol::ImageGenerations.supports_downstream("images_generations"));
        assert!(!UpstreamProtocol::ImageGenerations.supports_downstream("responses"));
    }

    fn provider_with_protocol_base_urls(
        base_url: &str,
        responses: Option<&str>,
        openai: Option<&str>,
        images_generations: Option<&str>,
    ) -> ProviderConfig {
        let mut provider = provider("openai_compatible");
        provider.base_url = base_url.to_string();
        provider.capabilities_json = Some(
            serde_json::to_string(&ProviderCapabilityOverrides {
                protocol_base_urls: Some(crate::domain::registry::ProviderProtocolBaseUrls {
                    responses: responses.map(str::to_string),
                    openai: openai.map(str::to_string),
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
            capabilities_json: None,
            models: Vec::new(),
            created_at: "2026-06-05T00:00:00Z".to_string(),
        }
    }
}
