use std::{borrow::Cow, collections::BTreeSet};

use reqwest::StatusCode;
use serde::Deserialize;

use crate::{
    models::ProviderConfig,
    providers::spec::{AuthScheme, ProviderSpec},
};

#[derive(Debug)]
pub enum ModelDiscoveryError {
    UpstreamStatus(StatusCode),
    RequestFailed,
    InvalidResponse,
}

impl ModelDiscoveryError {
    pub fn public_message(&self) -> String {
        match self {
            ModelDiscoveryError::UpstreamStatus(StatusCode::UNAUTHORIZED) => {
                "模型获取失败，上游认证失败，请检查 API Key。".to_string()
            }
            ModelDiscoveryError::UpstreamStatus(StatusCode::PAYMENT_REQUIRED) => {
                "模型获取失败，上游提示账户或额度状态受限。".to_string()
            }
            ModelDiscoveryError::UpstreamStatus(StatusCode::FORBIDDEN) => {
                "模型获取失败，上游拒绝列出模型，可能与套餐、权限或账户状态有关。".to_string()
            }
            ModelDiscoveryError::UpstreamStatus(status) => {
                format!("模型获取失败，上游返回状态码 {}", status.as_u16())
            }
            ModelDiscoveryError::RequestFailed => "模型获取失败，上游请求失败".to_string(),
            ModelDiscoveryError::InvalidResponse => "模型获取失败，上游响应格式不正确".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelItem>,
}

#[derive(Debug, Deserialize)]
struct ModelItem {
    id: String,
}

pub async fn discover_models(
    client: &reqwest::Client,
    provider_type: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, ModelDiscoveryError> {
    let mut last_error = None;
    for attempt in discovery_attempts(provider_type, base_url) {
        match request_models(client, &attempt, api_key.trim()).await {
            Ok(models) => return Ok(models),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or(ModelDiscoveryError::RequestFailed))
}

async fn request_models(
    client: &reqwest::Client,
    attempt: &DiscoveryAttempt<'_>,
    api_key: &str,
) -> Result<Vec<String>, ModelDiscoveryError> {
    let url = format!("{}/models", attempt.base_url.trim_end_matches('/'));
    let request = match attempt.auth_scheme {
        AuthScheme::Bearer => client.get(url).bearer_auth(api_key),
        AuthScheme::Anthropic => client
            .get(url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
    };

    let response = request
        .send()
        .await
        .map_err(|_| ModelDiscoveryError::RequestFailed)?;
    let status = response.status();
    if !status.is_success() {
        return Err(ModelDiscoveryError::UpstreamStatus(status));
    }
    let response = response
        .json::<ModelsResponse>()
        .await
        .map_err(|_| ModelDiscoveryError::InvalidResponse)?;
    Ok(normalize_models(response.data))
}

#[derive(Debug, PartialEq, Eq)]
struct DiscoveryAttempt<'a> {
    base_url: Cow<'a, str>,
    auth_scheme: AuthScheme,
}

fn discovery_attempts<'a>(provider_type: &str, base_url: &'a str) -> Vec<DiscoveryAttempt<'a>> {
    let base_url = base_url.trim();
    let mut attempts = vec![DiscoveryAttempt {
        base_url: Cow::Borrowed(base_url),
        auth_scheme: auth_scheme_for_provider_type(provider_type),
    }];

    if let Some(fallback_base_url) = models_fallback_base_url(provider_type, base_url) {
        if fallback_base_url.trim_end_matches('/') != base_url.trim_end_matches('/') {
            attempts.push(DiscoveryAttempt {
                base_url: Cow::Owned(fallback_base_url),
                auth_scheme: fallback_auth_scheme_for_provider_type(provider_type),
            });
        }
    }

    attempts
}

fn auth_scheme_for_provider_type(provider_type: &str) -> AuthScheme {
    let provider = ProviderConfig {
        id: String::new(),
        name: String::new(),
        provider_type: provider_type.trim().to_string(),
        base_url: String::new(),
        api_key: String::new(),
        token: String::new(),
        capabilities_json: None,
        created_at: String::new(),
    };
    ProviderSpec::from_provider_config(&provider).auth_scheme
}

fn models_fallback_base_url(provider_type: &str, base_url: &str) -> Option<String> {
    match provider_type.trim() {
        "deepseek_anthropic" => {
            replace_path_suffix(base_url, "/anthropic", "", "https://api.deepseek.com")
        }
        "minimax_anthropic" => Some(append_path_unless_present(base_url, "v1")),
        "kimi_coding_anthropic" => Some(append_path_unless_present(base_url, "v1")),
        "zai_coding_anthropic" => replace_path_suffix(
            base_url,
            "/api/anthropic",
            "/api/coding/paas/v4",
            "https://api.z.ai/api/coding/paas/v4",
        ),
        "zhipu_coding" => replace_path_suffix(
            base_url,
            "/api/anthropic",
            "/api/coding/paas/v4",
            "https://open.bigmodel.cn/api/coding/paas/v4",
        ),
        "bailian_coding_anthropic" => replace_path_suffix(
            base_url,
            "/apps/anthropic",
            "/v1",
            "https://coding.dashscope.aliyuncs.com/v1",
        ),
        "bailian_token_anthropic" => replace_path_suffix(
            base_url,
            "/apps/anthropic",
            "/compatible-mode/v1",
            "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
        ),
        _ => None,
    }
}

fn fallback_auth_scheme_for_provider_type(provider_type: &str) -> AuthScheme {
    match provider_type.trim() {
        "minimax_anthropic" => AuthScheme::Anthropic,
        _ => AuthScheme::Bearer,
    }
}

fn append_path_unless_present(base_url: &str, path: &str) -> String {
    let base_url = base_url.trim().trim_end_matches('/');
    let path = path.trim_matches('/');
    if base_url.ends_with(&format!("/{path}")) {
        base_url.to_string()
    } else {
        format!("{base_url}/{path}")
    }
}

fn replace_path_suffix(
    base_url: &str,
    suffix: &str,
    replacement: &str,
    default_base_url: &str,
) -> Option<String> {
    let base_url = base_url.trim().trim_end_matches('/');
    base_url
        .strip_suffix(suffix)
        .map(|prefix| format!("{prefix}{replacement}"))
        .or_else(|| Some(default_base_url.to_string()))
}

fn normalize_models(models: Vec<ModelItem>) -> Vec<String> {
    models
        .into_iter()
        .filter_map(|model| {
            let id = model.id.trim();
            (!id.is_empty()).then(|| id.to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use axum::{http::HeaderMap, routing::get, Json, Router};
    use reqwest::StatusCode;
    use serde_json::json;

    use super::{
        discover_models, discovery_attempts, AuthScheme, DiscoveryAttempt, ModelDiscoveryError,
    };

    #[test]
    fn explains_model_discovery_authorization_and_account_status_failures() {
        let cases = [
            (
                StatusCode::UNAUTHORIZED,
                "模型获取失败，上游认证失败，请检查 API Key。",
            ),
            (
                StatusCode::PAYMENT_REQUIRED,
                "模型获取失败，上游提示账户或额度状态受限。",
            ),
            (
                StatusCode::FORBIDDEN,
                "模型获取失败，上游拒绝列出模型，可能与套餐、权限或账户状态有关。",
            ),
        ];

        for (status, message) in cases {
            assert_eq!(
                ModelDiscoveryError::UpstreamStatus(status).public_message(),
                message
            );
        }
    }

    #[tokio::test]
    async fn falls_back_to_kimi_coding_openai_models_endpoint_for_anthropic_plan() {
        let app = Router::new().route(
            "/coding/v1/models",
            get(|headers: HeaderMap| async move {
                let auth_is_valid = headers
                    .get(reqwest::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    == Some("Bearer sk-coding-secret");
                assert!(auth_is_valid);
                Json(json!({
                    "data": [
                        { "id": "kimi-k2-0711-preview" },
                        { "id": "kimi-k2-0711-preview" },
                        { "id": "kimi-k2-turbo-preview" }
                    ]
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream server");
        let upstream_addr = listener.local_addr().expect("read upstream address");
        let upstream_server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });

        let models = discover_models(
            &reqwest::Client::new(),
            "kimi_coding_anthropic",
            &format!("http://{upstream_addr}/coding"),
            "sk-coding-secret",
        )
        .await
        .expect("discover models through fallback");

        assert_eq!(
            models,
            vec!["kimi-k2-0711-preview", "kimi-k2-turbo-preview"]
        );

        upstream_server.abort();
    }

    #[test]
    fn keeps_fallbacks_scoped_to_documented_anthropic_plan_pairs() {
        let cases = [
            (
                "kimi_coding_anthropic",
                "https://api.kimi.com/coding",
                "https://api.kimi.com/coding/v1",
            ),
            (
                "zai_coding_anthropic",
                "https://api.z.ai/api/anthropic",
                "https://api.z.ai/api/coding/paas/v4",
            ),
            (
                "zhipu_coding",
                "https://open.bigmodel.cn/api/anthropic",
                "https://open.bigmodel.cn/api/coding/paas/v4",
            ),
            (
                "bailian_coding_anthropic",
                "https://coding.dashscope.aliyuncs.com/apps/anthropic",
                "https://coding.dashscope.aliyuncs.com/v1",
            ),
            (
                "bailian_token_anthropic",
                "https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic",
                "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
            ),
        ];

        for (provider_type, base_url, fallback_url) in cases {
            let attempts = discovery_attempts(provider_type, base_url);

            assert_eq!(attempts.len(), 2, "{provider_type}");
            assert_eq!(
                attempts[1],
                DiscoveryAttempt {
                    base_url: fallback_url.into(),
                    auth_scheme: AuthScheme::Bearer,
                },
                "{provider_type}"
            );
        }

        assert_eq!(
            discovery_attempts("minimax_token", "https://api.minimax.io/anthropic").len(),
            1
        );
        assert_eq!(
            discovery_attempts("anthropic_compatible", "https://api.example.test/v1").len(),
            1
        );
    }

    #[test]
    fn falls_back_only_for_api_services_with_documented_model_list_pairs() {
        let cases = [
            (
                "deepseek_anthropic",
                "https://api.deepseek.com/anthropic",
                "https://api.deepseek.com",
                AuthScheme::Bearer,
            ),
            (
                "minimax_anthropic",
                "https://api.minimaxi.com/anthropic",
                "https://api.minimaxi.com/anthropic/v1",
                AuthScheme::Anthropic,
            ),
        ];

        for (provider_type, base_url, fallback_url, auth_scheme) in cases {
            let attempts = discovery_attempts(provider_type, base_url);

            assert_eq!(attempts.len(), 2, "{provider_type}");
            assert_eq!(
                attempts[1],
                DiscoveryAttempt {
                    base_url: fallback_url.into(),
                    auth_scheme,
                },
                "{provider_type}"
            );
        }

        assert_eq!(
            discovery_attempts(
                "qwen_anthropic",
                "https://dashscope.aliyuncs.com/apps/anthropic"
            )
            .len(),
            1
        );
        assert_eq!(
            discovery_attempts("zhipu_anthropic", "https://open.bigmodel.cn/api/anthropic").len(),
            1
        );
    }
}
