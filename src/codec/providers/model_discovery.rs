use std::collections::BTreeSet;

use reqwest::StatusCode;
use serde::Deserialize;

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
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, ModelDiscoveryError> {
    request_models(client, base_url, api_key.trim()).await
}

async fn request_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, ModelDiscoveryError> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let request = client.get(url).bearer_auth(api_key);

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

    use super::{discover_models, ModelDiscoveryError};

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
    async fn discovers_models_from_the_provider_models_endpoint() {
        let app = Router::new().route(
            "/coding/models",
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
            &format!("http://{upstream_addr}/coding"),
            "sk-coding-secret",
        )
        .await
        .expect("discover models");

        assert_eq!(
            models,
            vec!["kimi-k2-0711-preview", "kimi-k2-turbo-preview"]
        );

        upstream_server.abort();
    }
}
