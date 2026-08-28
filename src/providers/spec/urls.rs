use prelay_protocol::ProviderResponse;

use crate::models::ProviderConfig;

use super::UpstreamProtocol;

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
