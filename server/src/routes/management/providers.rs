use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use provider_relay_protocol::{CreateProviderRequest, ProviderResponse, UpdateProviderRequest};
use serde::{Deserialize, Serialize};

use crate::{
    error::AppError,
    providers::{
        model_discovery,
        spec::{provider_response_upstream_base_url, UpstreamProtocol},
    },
    AppState,
};

use super::auth::CurrentIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/providers", get(list).post(create))
        .route(
            "/providers/:provider_id",
            get(get_one).patch(update).delete(delete_one),
        )
        .route("/providers/:provider_id/ping", post(ping))
        .route(
            "/providers/:provider_id/discover-models",
            post(discover_models),
        )
        .route("/providers/:provider_id/test-protocol", post(test_protocol))
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

#[derive(Debug, Deserialize)]
struct TestProviderProtocolRequest {
    protocol: String,
    model: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProviderOperationResponse {
    ok: bool,
    protocol: Option<String>,
    latency_ms: Option<i64>,
    first_token_ms: Option<i64>,
    error: Option<String>,
    models: Option<Vec<String>>,
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
    let api_key = state
        .storage
        .decrypt_provider_key(&identity.id, &provider_id)
        .await?;
    let started_at = std::time::Instant::now();
    let response = match model_discovery::discover_models(
        &state.client,
        &provider.provider_type,
        &provider.base_url,
        &api_key,
    )
    .await
    {
        Ok(_) => ProviderOperationResponse {
            ok: true,
            protocol: None,
            latency_ms: Some(started_at.elapsed().as_millis() as i64),
            first_token_ms: None,
            error: None,
            models: None,
        },
        Err(error) => ProviderOperationResponse {
            ok: false,
            protocol: None,
            latency_ms: Some(started_at.elapsed().as_millis() as i64),
            first_token_ms: None,
            error: Some(error.public_message()),
            models: None,
        },
    };
    Ok(Json(response))
}

async fn discover_models(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
) -> Result<Json<ProviderOperationResponse>, AppError> {
    let provider = state
        .storage
        .get_provider(&identity.id, &provider_id)
        .await?;
    let api_key = state
        .storage
        .decrypt_provider_key(&identity.id, &provider_id)
        .await?;
    let models = model_discovery::discover_models(
        &state.client,
        &provider.provider_type,
        &provider.base_url,
        &api_key,
    )
    .await
    .map_err(|error| AppError::BadRequest(error.public_message()))?;
    state
        .storage
        .add_provider_models(&identity.id, &provider_id, &models)
        .await?;
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
    Extension(identity): Extension<CurrentIdentity>,
    Path(provider_id): Path<String>,
    Json(input): Json<TestProviderProtocolRequest>,
) -> Result<Json<ProviderOperationResponse>, AppError> {
    let provider = state
        .storage
        .get_provider(&identity.id, &provider_id)
        .await?;
    let model = input
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            provider
                .models
                .first()
                .map(|model| model.model_name.clone())
        })
        .ok_or_else(|| AppError::BadRequest("测试模型不能为空".to_string()))?;
    let api_key = state
        .storage
        .decrypt_provider_key(&identity.id, &provider_id)
        .await?;
    let protocol = input.protocol.trim();
    let upstream_protocol = UpstreamProtocol::from_capability_value(protocol)
        .ok_or_else(|| AppError::BadRequest("协议不支持".to_string()))?;
    let base_url = provider_response_upstream_base_url(&provider, upstream_protocol);
    let started_at = std::time::Instant::now();
    let response = send_protocol_test_request(
        &state.client,
        upstream_protocol,
        &base_url,
        &api_key,
        &model,
    )
    .await
    .map_err(|error| AppError::BadRequest(sanitize_protocol_test_error(error)))?;
    let latency_ms = Some(started_at.elapsed().as_millis() as i64);
    if !response.status().is_success() {
        return Ok(Json(ProviderOperationResponse {
            ok: false,
            protocol: Some(protocol.to_string()),
            latency_ms,
            first_token_ms: None,
            error: Some(format!("上游测试失败: {}", response.status().as_u16())),
            models: None,
        }));
    }
    let first_token_ms = first_response_byte_ms(response, started_at).await?;
    Ok(Json(ProviderOperationResponse {
        ok: true,
        protocol: Some(protocol.to_string()),
        latency_ms: Some(started_at.elapsed().as_millis() as i64),
        first_token_ms,
        error: None,
        models: None,
    }))
}

async fn send_protocol_test_request(
    client: &reqwest::Client,
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    match protocol {
        UpstreamProtocol::Responses => {
            client
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
        }
        UpstreamProtocol::ChatCompletions => {
            client
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
        }
        UpstreamProtocol::AnthropicMessages => {
            client
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
        }
    }
}

async fn first_response_byte_ms(
    response: reqwest::Response,
    started_at: std::time::Instant,
) -> Result<Option<i64>, AppError> {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    match stream.next().await {
        Some(Ok(_)) => Ok(Some(started_at.elapsed().as_millis() as i64)),
        Some(Err(error)) => Err(AppError::BadRequest(sanitize_protocol_test_error(error))),
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
