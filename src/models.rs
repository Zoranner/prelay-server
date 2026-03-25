use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    /// "openai" | "anthropic" | "minimax" | "minimax_token" | "zhipu" | "zhipu_coding" | "openai_compatible" | "anthropic_compatible"
    pub provider_type: String,
    /// API base URL, e.g. https://api.openai.com
    pub base_url: String,
    pub api_key: String,
    /// Internal token used by clients to authenticate with this proxy
    pub token: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateConfigRequest {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub name: Option<String>,
    pub provider_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    /// Masked API key, only first 4 and last 4 chars visible
    pub api_key_masked: String,
    pub token: String,
    pub created_at: String,
}

impl ProviderConfig {
    /// Returns true if this provider uses Anthropic-style auth
    /// (x-api-key + anthropic-version header) instead of Bearer.
    pub fn uses_anthropic_auth(&self) -> bool {
        matches!(
            self.provider_type.as_str(),
            "anthropic" | "minimax_token" | "zhipu_coding" | "anthropic_compatible"
        )
    }
}

impl From<ProviderConfig> for ConfigResponse {
    fn from(c: ProviderConfig) -> Self {
        let api_key_masked = mask_key(&c.api_key);
        ConfigResponse {
            id: c.id,
            name: c.name,
            provider_type: c.provider_type,
            base_url: c.base_url,
            api_key_masked,
            token: c.token,
            created_at: c.created_at,
        }
    }
}

fn mask_key(key: &str) -> String {
    let len = key.len();
    if len <= 8 {
        return "*".repeat(len);
    }
    format!("{}...{}", &key[..4], &key[len - 4..])
}
