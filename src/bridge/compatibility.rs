use serde_json::json;

use crate::{
    bridge::internal::{InternalRequest, InternalRole},
    models::ProviderConfig,
    providers::spec::ProviderSpec,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityRejection {
    pub field: &'static str,
    pub reason: &'static str,
}

impl CompatibilityRejection {
    pub fn error_code(self) -> &'static str {
        "compatibility_rejected"
    }

    pub fn metadata_json(self) -> String {
        json!({
            "decision": "rejected",
            "field": self.field,
            "reason": self.reason
        })
        .to_string()
    }
}

pub fn first_rejection(
    provider: &ProviderConfig,
    request: &InternalRequest,
) -> Option<CompatibilityRejection> {
    let spec = ProviderSpec::from_provider_config(provider);
    if !request.tools.is_empty() && !spec.capabilities.tool_calls {
        return Some(CompatibilityRejection {
            field: "tools",
            reason: "provider does not advertise tool call support",
        });
    }
    if request.tool_choice_requested && !spec.capabilities.tool_choice {
        return Some(CompatibilityRejection {
            field: "tool_choice",
            reason: "provider does not advertise tool choice support",
        });
    }
    if request.reasoning_requested && !spec.capabilities.reasoning {
        return Some(CompatibilityRejection {
            field: "reasoning",
            reason: "provider does not advertise reasoning support",
        });
    }
    if request.structured_output_requested && !spec.capabilities.structured_outputs {
        return Some(CompatibilityRejection {
            field: "structured_outputs",
            reason: "provider does not advertise structured output support",
        });
    }
    if request.parallel_tool_calls_requested && !spec.capabilities.parallel_tool_calls {
        return Some(CompatibilityRejection {
            field: "parallel_tool_calls",
            reason: "provider does not advertise parallel tool call support",
        });
    }
    if request.streaming_usage_requested && !spec.capabilities.streaming_usage {
        return Some(CompatibilityRejection {
            field: "streaming_usage",
            reason: "provider does not advertise streaming usage support",
        });
    }
    if request
        .messages
        .iter()
        .any(|message| matches!(message.role, InternalRole::System))
        && !spec.capabilities.system_messages
    {
        return Some(CompatibilityRejection {
            field: "system_messages",
            reason: "provider does not advertise system message support",
        });
    }
    if let (Some(max_tokens), Some(limit)) =
        (request.max_tokens, spec.capabilities.max_output_tokens)
    {
        if max_tokens > limit {
            return Some(CompatibilityRejection {
                field: "max_tokens",
                reason: "request exceeds provider max output tokens",
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::first_rejection;
    use crate::{
        bridge::internal::{InternalRequest, InternalTool},
        models::ProviderConfig,
    };

    #[test]
    fn rejects_tools_when_provider_does_not_support_tool_calls() {
        let provider = provider("ollama_native");
        let request = request_with_tool();

        let rejection = first_rejection(&provider, &request).expect("rejection");

        assert_eq!(rejection.field, "tools");
        assert_eq!(
            rejection.reason,
            "provider does not advertise tool call support"
        );
    }

    #[test]
    fn allows_tools_when_provider_supports_tool_calls() {
        let provider = provider("openai_compatible");
        let request = request_with_tool();

        assert!(first_rejection(&provider, &request).is_none());
    }

    #[test]
    fn rejects_reasoning_when_provider_does_not_advertise_support() {
        let provider = provider("openai_compatible");
        let mut request = request_with_tool();
        request.tools = Vec::new();
        request.reasoning_requested = true;

        let rejection = first_rejection(&provider, &request).expect("rejection");

        assert_eq!(rejection.field, "reasoning");
    }

    #[test]
    fn rejects_max_tokens_above_provider_limit() {
        let mut provider = provider("openai_compatible");
        provider.capabilities_json = Some(
            serde_json::json!({
                "max_output_tokens": 128
            })
            .to_string(),
        );
        let mut request = request_with_tool();
        request.tools = Vec::new();
        request.max_tokens = Some(256);

        let rejection = first_rejection(&provider, &request).expect("rejection");

        assert_eq!(rejection.field, "max_tokens");
    }

    #[test]
    fn rejects_parallel_tool_calls_when_provider_does_not_advertise_support() {
        let provider = provider("ollama_native");
        let mut request = request_with_tool();
        request.tools = Vec::new();
        request.parallel_tool_calls_requested = true;

        let rejection = first_rejection(&provider, &request).expect("rejection");

        assert_eq!(rejection.field, "parallel_tool_calls");
    }

    #[test]
    fn rejects_streaming_usage_when_provider_does_not_advertise_support() {
        let provider = provider("openai_compatible");
        let mut request = request_with_tool();
        request.tools = Vec::new();
        request.streaming_usage_requested = true;

        let rejection = first_rejection(&provider, &request).expect("rejection");

        assert_eq!(rejection.field, "streaming_usage");
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

    fn request_with_tool() -> InternalRequest {
        InternalRequest {
            model: "model".to_string(),
            stream: false,
            max_tokens: None,
            previous_response_id: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
            parallel_tool_calls_requested: false,
            streaming_usage_requested: false,
            tools: vec![InternalTool {
                name: "read_file".to_string(),
                description: None,
                input_schema: json!({ "type": "object" }),
            }],
            messages: Vec::new(),
        }
    }
}
