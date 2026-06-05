use serde_json::json;

use crate::{bridge::internal::InternalRequest, models::ProviderConfig};

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
    if !request.tools.is_empty() && !supports_tool_calls(&provider.provider_type) {
        return Some(CompatibilityRejection {
            field: "tools",
            reason: "provider does not advertise tool call support",
        });
    }

    None
}

fn supports_tool_calls(provider_type: &str) -> bool {
    matches!(
        provider_type,
        "openai" | "openai_compatible" | "anthropic" | "anthropic_compatible"
    )
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

    fn request_with_tool() -> InternalRequest {
        InternalRequest {
            model: "model".to_string(),
            stream: false,
            max_tokens: None,
            previous_response_id: None,
            tools: vec![InternalTool {
                name: "read_file".to_string(),
                description: None,
                input_schema: json!({ "type": "object" }),
            }],
            messages: Vec::new(),
        }
    }
}
