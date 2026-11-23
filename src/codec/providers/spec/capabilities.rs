use prelay_protocol::ProviderProtocol;

use crate::{domain::catalog::ProviderCatalog, domain::registry::ProviderConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamProtocol {
    Responses,
    ChatCompletions,
    ImageGenerations,
}

impl UpstreamProtocol {
    pub fn downstream_protocols(self) -> &'static [&'static str] {
        match self {
            UpstreamProtocol::Responses => &["responses"],
            UpstreamProtocol::ChatCompletions => &["responses", "chat_completions"],
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
            "images_generations" => Some(UpstreamProtocol::ImageGenerations),
            _ => None,
        }
    }

    fn capability_value(self) -> &'static str {
        match self {
            UpstreamProtocol::Responses => "responses",
            UpstreamProtocol::ChatCompletions => "openai",
            UpstreamProtocol::ImageGenerations => "images_generations",
        }
    }

    pub(crate) fn from_catalog(protocol: ProviderProtocol) -> Self {
        match protocol {
            ProviderProtocol::Responses => UpstreamProtocol::Responses,
            ProviderProtocol::ChatCompletions => UpstreamProtocol::ChatCompletions,
            ProviderProtocol::ImagesGenerations => UpstreamProtocol::ImageGenerations,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    pub supported_protocols: Vec<UpstreamProtocol>,
}

impl ProviderSpec {
    pub fn from_provider_config(catalog: &ProviderCatalog, provider: &ProviderConfig) -> Self {
        Self {
            supported_protocols: catalog_protocols(catalog, &provider.provider_type),
        }
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
            ][..],
            "chat_completions" => &[UpstreamProtocol::ChatCompletions][..],
            "images_generations" => &[UpstreamProtocol::ImageGenerations][..],
            _ => &[][..],
        };
        preference
            .iter()
            .copied()
            .find(|protocol| self.supported_protocols.contains(protocol))
    }
}

/// 上游协议只由目录定义；目录里没有这家供应商时返回空集合。
pub fn resolved_upstream_protocols(catalog: &ProviderCatalog, provider_type: &str) -> Vec<String> {
    catalog_protocols(catalog, provider_type)
        .into_iter()
        .map(UpstreamProtocol::capability_value)
        .map(str::to_owned)
        .collect()
}

fn catalog_protocols(catalog: &ProviderCatalog, provider_type: &str) -> Vec<UpstreamProtocol> {
    catalog
        .provider(provider_type)
        .map(|provider| {
            provider
                .protocols
                .iter()
                .copied()
                .map(UpstreamProtocol::from_catalog)
                .collect()
        })
        .unwrap_or_default()
}
