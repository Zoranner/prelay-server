use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    /// Stable provider catalog id, such as "openai" or "kimi_coding".
    pub provider_type: String,
    /// API base URL, e.g. https://api.openai.com
    pub base_url: String,
    pub api_key: String,
    /// Internal token used by clients to authenticate with this proxy
    pub token: String,
    pub capabilities_json: Option<String>,
    /// 该供应商启用的模型清单；清单外的模型不再提供给接入点。
    pub models: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct EndpointModel {
    pub id: String,
    pub endpoint_id: String,
    pub model_name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilityOverrides {
    pub upstream_protocols: Option<Vec<String>>,
    pub protocol_base_urls: Option<ProviderProtocolBaseUrls>,
    pub tool_calls: Option<bool>,
    pub reasoning: Option<bool>,
    pub tool_choice: Option<bool>,
    pub parallel_tool_calls: Option<bool>,
    pub system_messages: Option<bool>,
    pub structured_outputs: Option<bool>,
    pub streaming_usage: Option<bool>,
    pub max_context_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderProtocolBaseUrls {
    pub responses: Option<String>,
    pub openai: Option<String>,
    pub images_generations: Option<String>,
}

impl ProviderConfig {
    pub fn provides_model(&self, model_id: &str) -> bool {
        self.models.iter().any(|id| id == model_id)
    }

    pub fn capability_overrides(&self) -> ProviderCapabilityOverrides {
        self.capabilities_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default()
    }
}
