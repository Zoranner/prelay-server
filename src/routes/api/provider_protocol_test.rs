use prelay_protocol::ProviderOperationResponse;

use crate::{
    error::AppError,
    providers::spec::{normalize_upstream_base_url, UpstreamProtocol},
};

pub(crate) async fn run_protocol_test(
    client: &reqwest::Client,
    provider_type: &str,
    protocol_value: Option<&str>,
    base_url: &str,
    api_key: &str,
    model_value: Option<&str>,
) -> Result<ProviderOperationResponse, AppError> {
    let protocol = protocol_value.unwrap_or_default().trim();
    let upstream_protocol = UpstreamProtocol::from_capability_value(protocol)
        .ok_or_else(|| AppError::BadRequest("协议不支持".to_string()))?;
    if upstream_protocol == UpstreamProtocol::ImageGenerations {
        return Err(AppError::BadRequest(
            "Images Generations 协议不支持连通性测试".to_string(),
        ));
    }
    let model = model_value
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| AppError::BadRequest("测试模型不能为空".to_string()))?;
    let base_url = normalize_upstream_base_url(provider_type, upstream_protocol, base_url);
    let started_at = std::time::Instant::now();
    let response = match send_protocol_test_request(
        client,
        upstream_protocol,
        &base_url,
        api_key,
        model,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            return Ok(ProviderOperationResponse {
                ok: false,
                protocol: Some(protocol.to_string()),
                latency_ms: Some(started_at.elapsed().as_millis() as i64),
                first_token_ms: None,
                error: Some(sanitize_protocol_test_error(error)),
                models: None,
            });
        }
    };
    let Some(response) = response else {
        return Err(AppError::BadRequest(
            "Images Generations 协议不支持连通性测试".to_string(),
        ));
    };
    let latency_ms = Some(started_at.elapsed().as_millis() as i64);
    if !response.status().is_success() {
        return Ok(ProviderOperationResponse {
            ok: false,
            protocol: Some(protocol.to_string()),
            latency_ms,
            first_token_ms: None,
            error: Some(format!("上游测试失败: {}", response.status().as_u16())),
            models: None,
        });
    }
    let first_token_ms = match first_response_byte_ms(response, started_at).await {
        Ok(first_token_ms) => first_token_ms,
        Err(error) => {
            return Ok(ProviderOperationResponse {
                ok: false,
                protocol: Some(protocol.to_string()),
                latency_ms: Some(started_at.elapsed().as_millis() as i64),
                first_token_ms: None,
                error: Some(sanitize_protocol_test_error(error)),
                models: None,
            });
        }
    };
    Ok(ProviderOperationResponse {
        ok: true,
        protocol: Some(protocol.to_string()),
        latency_ms,
        first_token_ms,
        error: None,
        models: None,
    })
}

async fn send_protocol_test_request(
    client: &reqwest::Client,
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<Option<reqwest::Response>, reqwest::Error> {
    match protocol {
        UpstreamProtocol::Responses => client
            .post(format!("{}/responses", base_url.trim_end_matches('/')))
            .bearer_auth(api_key)
            .json(&serde_json::json!({
                "model": model,
                "stream": true,
                "input": [{ "role": "user", "content": "ping" }],
                "max_output_tokens": 8
            }))
            .send()
            .await
            .map(Some),
        UpstreamProtocol::ChatCompletions => client
            .post(format!(
                "{}/chat/completions",
                base_url.trim_end_matches('/')
            ))
            .bearer_auth(api_key)
            .json(&serde_json::json!({
                "model": model,
                "stream": true,
                "messages": [{ "role": "user", "content": "ping" }],
                "max_tokens": 8
            }))
            .send()
            .await
            .map(Some),
        UpstreamProtocol::AnthropicMessages => client
            .post(format!("{}/messages", base_url.trim_end_matches('/')))
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&serde_json::json!({
                "model": model,
                "stream": true,
                "messages": [{ "role": "user", "content": "ping" }],
                "max_tokens": 8
            }))
            .send()
            .await
            .map(Some),
        UpstreamProtocol::ImageGenerations => Ok(None),
    }
}

async fn first_response_byte_ms(
    response: reqwest::Response,
    started_at: std::time::Instant,
) -> Result<Option<i64>, reqwest::Error> {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    match stream.next().await {
        Some(Ok(_)) => Ok(Some(started_at.elapsed().as_millis() as i64)),
        Some(Err(error)) => Err(error),
        None => Ok(None),
    }
}

fn sanitize_protocol_test_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        return "上游测试超时".to_string();
    }
    if error.is_connect() {
        return "上游连接失败".to_string();
    }
    "上游测试失败".to_string()
}

#[cfg(test)]
mod tests {
    use super::run_protocol_test;
    use crate::error::AppError;

    #[tokio::test]
    async fn rejects_images_generations_protocol_tests_without_contacting_upstream() {
        let error = run_protocol_test(
            &reqwest::Client::new(),
            "openai_compatible",
            Some("images_generations"),
            "http://127.0.0.1:1",
            "test-key",
            Some("test-model"),
        )
        .await
        .expect_err("image generation protocol tests must be rejected");

        assert!(matches!(
            error,
            AppError::BadRequest(message) if message == "Images Generations 协议不支持连通性测试"
        ));
    }

    #[tokio::test]
    async fn rejects_images_generations_protocol_tests_without_a_model() {
        let error = run_protocol_test(
            &reqwest::Client::new(),
            "openai_compatible",
            Some("images_generations"),
            "http://127.0.0.1:1",
            "test-key",
            None,
        )
        .await
        .expect_err("image generation protocol tests must be rejected before model validation");

        assert!(matches!(
            error,
            AppError::BadRequest(message) if message == "Images Generations 协议不支持连通性测试"
        ));
    }
}
