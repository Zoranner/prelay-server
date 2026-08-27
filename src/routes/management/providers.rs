use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use prelay_protocol::{
    CreateProviderRequest, ProviderOperationRequest, ProviderOperationResponse, ProviderResponse,
    UpdateProviderRequest,
};

use crate::{
    error::AppError,
    providers::{
        model_discovery,
        spec::{normalize_upstream_base_url, UpstreamProtocol},
    },
    AppState,
};

use super::auth::CurrentIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/providers", get(list).post(create))
        .route("/providers/discover-models", post(discover_models))
        .route("/providers/test-protocol", post(test_protocol))
        .route("/providers/:provider_id/ping", post(ping))
        .route(
            "/providers/:provider_id",
            get(get_one).patch(update).delete(delete_one),
        )
}

async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<Vec<ProviderResponse>>, AppError> {
    Ok(Json(state.storage.list_providers(&identity.id).await?))
}

async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Json(input): Json<CreateProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), AppError> {
    let provider_id = state.storage.create_provider(&identity.id, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .storage
                .get_provider(&identity.id, &provider_id)
                .await?,
        ),
    ))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<Json<ProviderResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .get_provider(&identity.id, &provider_id)
            .await?,
    ))
}

async fn update(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
    Json(input): Json<UpdateProviderRequest>,
) -> Result<Json<ProviderResponse>, AppError> {
    Ok(Json(
        state
            .storage
            .update_provider(&identity.id, &provider_id, input)
            .await?,
    ))
}

async fn delete_one(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<StatusCode, AppError> {
    state
        .storage
        .delete_provider(&identity.id, &provider_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ping(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<Json<ProviderOperationResponse>, AppError> {
    let provider = state
        .storage
        .get_provider(&identity.id, &provider_id)
        .await?;
    let started_at = std::time::Instant::now();
    let response = state.client.head(&provider.base_url).send().await;
    let latency_ms = Some(started_at.elapsed().as_millis() as i64);

    Ok(Json(match response {
        Ok(_) => ProviderOperationResponse {
            ok: true,
            protocol: None,
            latency_ms,
            first_token_ms: None,
            error: None,
            models: None,
        },
        Err(error) => ProviderOperationResponse {
            ok: false,
            protocol: None,
            latency_ms,
            first_token_ms: None,
            error: Some(if error.is_timeout() {
                "上游连接超时".to_string()
            } else {
                "上游连接失败".to_string()
            }),
            models: None,
        },
    }))
}

async fn discover_models(
    State(state): State<AppState>,
    Json(input): Json<ProviderOperationRequest>,
) -> Result<Json<ProviderOperationResponse>, AppError> {
    let models = match model_discovery::discover_models(
        &state.client,
        &input.provider_type,
        &input.base_url,
        &input.api_key,
    )
    .await
    {
        Ok(models) => models,
        Err(error) => {
            return Ok(Json(ProviderOperationResponse {
                ok: false,
                protocol: None,
                latency_ms: None,
                first_token_ms: None,
                error: Some(error.public_message()),
                models: None,
            }));
        }
    };
    Ok(Json(ProviderOperationResponse {
        ok: true,
        protocol: None,
        latency_ms: None,
        first_token_ms: None,
        error: None,
        models: Some(models),
    }))
}

async fn test_protocol(
    State(state): State<AppState>,
    Json(input): Json<ProviderOperationRequest>,
) -> Result<Json<ProviderOperationResponse>, AppError> {
    Ok(Json(
        run_protocol_test(
            &state.client,
            &input.provider_type,
            input.protocol.as_deref(),
            &input.base_url,
            &input.api_key,
            input.model.as_deref(),
        )
        .await?,
    ))
}

async fn run_protocol_test(
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
            "图像生成协议不支持连通性测试".to_string(),
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
        latency_ms: Some(started_at.elapsed().as_millis() as i64),
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
    async fn rejects_image_generation_protocol_tests_without_contacting_upstream() {
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
            AppError::BadRequest(message) if message == "图像生成协议不支持连通性测试"
        ));
    }
}
