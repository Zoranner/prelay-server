use crate::models::{ProviderCapabilityOverrides, ProviderConfig};

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
            if let [protocol] = protocols.as_slice() {
                spec.protocol = *protocol;
            }
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
