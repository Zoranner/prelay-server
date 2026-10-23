use crate::models::ProviderConfig;

use super::UpstreamProtocol;

/// 供应商记录里为该协议设置了地址就用设置，没有设置就用 base_url。
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
                UpstreamProtocol::ImageGenerations => base_urls.images_generations.as_deref(),
            });

    let protocol_base_url = protocol_base_url
        .map(str::trim)
        .filter(|base_url| !base_url.is_empty());
    normalize_upstream_base_url(protocol_base_url.unwrap_or(provider.base_url.trim()))
}

pub fn normalize_upstream_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}
