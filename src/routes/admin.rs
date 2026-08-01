use std::collections::HashSet;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    db,
    error::AppError,
    models::{
        ConfigResponse, CreateConfigRequest, CreateInterfaceModelRequest, CreateInterfaceRequest,
        CreateProviderModelRequest, DiscoverModelsRequest, DiscoverModelsResponse,
        InterfaceModelInput, InterfaceResponse, PingProviderResponse, ProviderModelResponse,
        TestProviderProtocolRequest, TestProviderProtocolResponse, UpdateConfigRequest,
        UpdateInterfaceRequest,
    },
    models::{CreateModelAliasRequest, ModelAliasResponse},
    providers::model_discovery,
    providers::spec::{normalize_upstream_base_url, ProviderSpec, UpstreamProtocol},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/configs", get(list_configs).post(create_config))
        .route("/configs/discover-models", post(discover_unsaved_models))
        .route("/configs/test-protocol", post(test_unsaved_protocol))
        .route("/configs/by-token/:token", get(get_config_by_token))
        .route("/configs/:id", put(update_config).delete(delete_config))
        .route("/configs/:id/regenerate-token", post(regenerate_token))
        .route("/configs/:id/discover-models", post(discover_saved_models))
        .route("/configs/:id/ping", post(ping_saved_provider))
        .route("/configs/:id/test-protocol", post(test_saved_protocol))
        .route(
            "/configs/:id/models",
            get(list_provider_models).post(create_provider_model),
        )
        .route(
            "/configs/:id/models/:model_id",
            delete(delete_provider_model),
        )
        .route(
            "/model-aliases",
            get(list_model_aliases)
                .post(create_model_alias)
                .delete(delete_model_alias_protocol),
        )
        .route("/interfaces", get(list_interfaces).post(create_interface))
        .route(
            "/interfaces/:id",
            put(update_interface).delete(delete_interface),
        )
        .route(
            "/interfaces/:id/regenerate-token",
            post(regenerate_interface_token),
        )
        .route("/interfaces/:id/models", post(create_interface_model))
        .route(
            "/interfaces/:interface_id/models/:model_id",
            delete(delete_interface_model),
        )
}

#[derive(Debug, Deserialize)]
struct DeleteModelAliasProtocolQuery {
    alias: String,
    downstream_protocol: String,
}

async fn get_config_by_token(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let config = db::get_config_by_token(&state.db, &token)
        .await?
        .ok_or_else(|| AppError::NotFound("密钥不存在".to_string()))?;
    let models = db::list_provider_models_by_provider(&state.db, &config.id).await?;
    Ok(Json(ConfigResponse::from_config(config, models)))
}

async fn list_configs(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let configs = db::list_configs(&state.db).await?;
    let models = db::list_provider_models(&state.db).await?;
    let responses: Vec<ConfigResponse> = configs
        .into_iter()
        .map(|config| {
            let provider_models = models
                .iter()
                .filter(|model| model.provider_id == config.id)
                .cloned()
                .collect::<Vec<_>>();
            ConfigResponse::from_config(config, provider_models)
        })
        .collect();
    Ok(Json(responses))
}

async fn list_model_aliases(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let aliases = db::list_model_aliases(&state.db).await?;
    let responses: Vec<ModelAliasResponse> = aliases.into_iter().map(Into::into).collect();
    Ok(Json(responses))
}

async fn list_interfaces(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let interfaces = db::list_interfaces(&state.db).await?;
    let models = db::list_interface_models(&state.db).await?;
    let responses = interfaces
        .into_iter()
        .map(|interface| {
            let interface_models = models
                .iter()
                .filter(|model| model.interface_id == interface.id)
                .cloned()
                .collect::<Vec<_>>();
            InterfaceResponse::from_config(interface, interface_models)
        })
        .collect::<Vec<_>>();
    Ok(Json(responses))
}

async fn create_config(
    State(state): State<AppState>,
    Json(req): Json<CreateConfigRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("名称不能为空".to_string()));
    }
    if req.api_key.trim().is_empty() {
        return Err(AppError::BadRequest("API Key 不能为空".to_string()));
    }
    if req.base_url.trim().is_empty() {
        return Err(AppError::BadRequest("Base URL 不能为空".to_string()));
    }
    let models = normalize_model_names(&req.models)?;

    let (config, models) = db::create_config_with_models(
        &state.db,
        &req.name,
        &req.provider_type,
        &req.base_url,
        &req.api_key,
        req.capabilities.as_ref(),
        &models,
    )
    .await?;

    let response = ConfigResponse::from_config(config, models);
    Ok((StatusCode::CREATED, Json(response)))
}

async fn update_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<impl IntoResponse, AppError> {
    let models = req
        .models
        .as_deref()
        .map(normalize_model_names)
        .transpose()?;
    let updated = db::update_config_with_models(&state.db, &id, &req, models.as_deref()).await?;

    let (config, models) =
        updated.ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    Ok(Json(ConfigResponse::from_config(config, models)))
}

fn normalize_model_names(model_names: &[String]) -> Result<Vec<String>, AppError> {
    let mut normalized = Vec::with_capacity(model_names.len());
    let mut seen = std::collections::HashSet::with_capacity(model_names.len());
    for model_name in model_names {
        let model_name = model_name.trim();
        if model_name.is_empty() {
            return Err(AppError::BadRequest("模型名称不能为空".to_string()));
        }
        if !seen.insert(model_name.to_string()) {
            return Err(AppError::BadRequest(format!(
                "模型名称 {} 重复",
                model_name
            )));
        }
        normalized.push(model_name.to_string());
    }
    Ok(normalized)
}

async fn delete_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = db::delete_config(&state.db, &id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("配置 {} 不存在", id)));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn regenerate_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let token = db::regenerate_token(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    Ok(Json(serde_json::json!({ "token": token })))
}

async fn discover_unsaved_models(
    State(state): State<AppState>,
    Json(req): Json<DiscoverModelsRequest>,
) -> Result<impl IntoResponse, AppError> {
    validate_discover_models_input(&req.provider_type, &req.base_url, &req.api_key)?;
    let models = model_discovery::discover_models(
        &state.client,
        req.provider_type.trim(),
        req.base_url.trim(),
        req.api_key.trim(),
    )
    .await
    .map_err(|error| AppError::BadRequest(error.public_message()))?;
    Ok(Json(DiscoverModelsResponse { models }))
}

async fn discover_saved_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let config = db::get_config_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    let models = model_discovery::discover_models(
        &state.client,
        &config.provider_type,
        &config.base_url,
        &config.api_key,
    )
    .await
    .map_err(|error| AppError::BadRequest(error.public_message()))?;
    db::upsert_provider_models(&state.db, &config.id, &models).await?;
    Ok(Json(DiscoverModelsResponse { models }))
}

async fn ping_saved_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let config = db::get_config_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    let started_at = std::time::Instant::now();
    let response = match model_discovery::discover_models(
        &state.client,
        &config.provider_type,
        &config.base_url,
        &config.api_key,
    )
    .await
    {
        Ok(_) => PingProviderResponse {
            ok: true,
            latency_ms: started_at.elapsed().as_millis() as i64,
            error: None,
        },
        Err(error) => PingProviderResponse {
            ok: false,
            latency_ms: started_at.elapsed().as_millis() as i64,
            error: Some(error.public_message()),
        },
    };
    Ok(Json(response))
}

async fn test_unsaved_protocol(
    State(state): State<AppState>,
    Json(req): Json<TestProviderProtocolRequest>,
) -> Result<impl IntoResponse, AppError> {
    let api_key = req
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| AppError::BadRequest("API Key 不能为空".to_string()))?;
    let model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| AppError::BadRequest("测试模型不能为空".to_string()))?;
    let response = test_provider_protocol(
        &state.client,
        &req.provider_type,
        &req.protocol,
        &req.base_url,
        api_key,
        model,
    )
    .await?;
    Ok(Json(response))
}

async fn test_saved_protocol(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<TestProviderProtocolRequest>,
) -> Result<impl IntoResponse, AppError> {
    let config = db::get_config_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    let owned_model;
    let model = if let Some(model) = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        model
    } else {
        let models = db::list_provider_models_by_provider(&state.db, &id).await?;
        owned_model = models
            .first()
            .map(|model| model.model_name.clone())
            .ok_or_else(|| AppError::BadRequest("测试模型不能为空".to_string()))?;
        &owned_model
    };
    let provider_type = req.provider_type.trim();
    let provider_type = if provider_type.is_empty() {
        config.provider_type.as_str()
    } else {
        provider_type
    };
    let response = test_provider_protocol(
        &state.client,
        provider_type,
        &req.protocol,
        &req.base_url,
        &config.api_key,
        model,
    )
    .await?;
    Ok(Json(response))
}

fn validate_discover_models_input(
    provider_type: &str,
    base_url: &str,
    api_key: &str,
) -> Result<(), AppError> {
    if provider_type.trim().is_empty() {
        return Err(AppError::BadRequest("Provider Type 不能为空".to_string()));
    }
    if base_url.trim().is_empty() {
        return Err(AppError::BadRequest("Base URL 不能为空".to_string()));
    }
    if api_key.trim().is_empty() {
        return Err(AppError::BadRequest("API Key 不能为空".to_string()));
    }
    Ok(())
}

async fn test_provider_protocol(
    client: &reqwest::Client,
    provider_type: &str,
    protocol: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<TestProviderProtocolResponse, AppError> {
    let provider_type = provider_type.trim();
    let protocol = protocol.trim();
    let base_url = base_url.trim();
    let api_key = api_key.trim();
    let model = model.trim();
    if provider_type.is_empty() {
        return Err(AppError::BadRequest("Provider Type 不能为空".to_string()));
    }
    if base_url.is_empty() {
        return Err(AppError::BadRequest("Base URL 不能为空".to_string()));
    }
    if api_key.is_empty() {
        return Err(AppError::BadRequest("API Key 不能为空".to_string()));
    }
    if model.is_empty() {
        return Err(AppError::BadRequest("测试模型不能为空".to_string()));
    }
    let upstream_protocol = UpstreamProtocol::from_capability_value(protocol)
        .ok_or_else(|| AppError::BadRequest("协议不支持".to_string()))?;
    let base_url = normalize_upstream_base_url(provider_type, upstream_protocol, base_url);
    let started_at = std::time::Instant::now();
    let response = send_protocol_test_request(client, upstream_protocol, &base_url, api_key, model)
        .await
        .map_err(|error| AppError::BadRequest(sanitize_protocol_test_error(error)))?;
    let status = response.status();
    if !status.is_success() {
        return Ok(TestProviderProtocolResponse {
            ok: false,
            protocol: protocol.to_string(),
            latency_ms: started_at.elapsed().as_millis() as i64,
            first_token_ms: None,
            error: Some(format!("上游测试失败: {}", status.as_u16())),
        });
    }
    let first_token_ms = first_response_byte_ms(response, started_at).await?;
    Ok(TestProviderProtocolResponse {
        ok: true,
        protocol: protocol.to_string(),
        latency_ms: started_at.elapsed().as_millis() as i64,
        first_token_ms,
        error: None,
    })
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
            let url = format!("{}/responses", base_url.trim_end_matches('/'));
            client
                .post(url)
                .bearer_auth(api_key)
                .json(&serde_json::json!({
                    "model": model,
                    "stream": true,
                    "input": [
                        {
                            "role": "user",
                            "content": "ping"
                        }
                    ],
                    "max_output_tokens": 8
                }))
                .send()
                .await
        }
        UpstreamProtocol::ChatCompletions => {
            let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
            client
                .post(url)
                .bearer_auth(api_key)
                .json(&serde_json::json!({
                    "model": model,
                    "stream": true,
                    "messages": [
                        {
                            "role": "user",
                            "content": "ping"
                        }
                    ],
                    "max_tokens": 8
                }))
                .send()
                .await
        }
        UpstreamProtocol::AnthropicMessages => {
            let url = format!("{}/messages", base_url.trim_end_matches('/'));
            client
                .post(url)
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&serde_json::json!({
                    "model": model,
                    "stream": true,
                    "messages": [
                        {
                            "role": "user",
                            "content": "ping"
                        }
                    ],
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

async fn list_provider_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    db::get_config_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    let models = db::list_provider_models_by_provider(&state.db, &id).await?;
    let responses = models
        .into_iter()
        .map(ProviderModelResponse::from)
        .collect::<Vec<_>>();
    Ok(Json(responses))
}

async fn create_provider_model(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateProviderModelRequest>,
) -> Result<impl IntoResponse, AppError> {
    db::get_config_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("配置 {} 不存在", id)))?;
    let model_name = req.model_name.trim();
    if model_name.is_empty() {
        return Err(AppError::BadRequest("模型名称不能为空".to_string()));
    }
    if db::provider_model_exists(&state.db, &id, model_name).await? {
        return Err(AppError::BadRequest(format!("模型 {model_name} 已存在")));
    }
    let model = db::create_provider_model(&state.db, &id, model_name).await?;
    Ok((
        StatusCode::CREATED,
        Json(ProviderModelResponse::from(model)),
    ))
}

async fn delete_provider_model(
    State(state): State<AppState>,
    Path((id, model_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = db::delete_provider_model(&state.db, &id, &model_id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!(
            "供应商模型 {} 不存在",
            model_id
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn create_interface(
    State(state): State<AppState>,
    Json(req): Json<CreateInterfaceRequest>,
) -> Result<impl IntoResponse, AppError> {
    validate_interface_name(&req.name)?;
    let protocol = req.protocol.as_deref().map(str::trim).unwrap_or("all");
    let models = normalize_interface_models(&req.models)?;
    let (interface, models) =
        db::create_interface_with_models(&state.db, req.name.trim(), protocol, &models)
            .await
            .map_err(|error| map_interface_write_error(error, None))?;
    Ok((
        StatusCode::CREATED,
        Json(InterfaceResponse::from_config(interface, models)),
    ))
}

async fn update_interface(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateInterfaceRequest>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(name) = req.name.as_deref() {
        validate_interface_name(name)?;
    }
    let models = req
        .models
        .as_deref()
        .map(normalize_interface_models)
        .transpose()?;
    let (interface, models) = db::update_interface_with_models(
        &state.db,
        &id,
        req.name.as_deref().map(str::trim),
        models.as_deref(),
    )
    .await
    .map_err(|error| map_interface_write_error(error, Some(&id)))?;
    Ok(Json(InterfaceResponse::from_config(interface, models)))
}

async fn delete_interface(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = db::delete_interface(&state.db, &id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("接口 {} 不存在", id)));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn regenerate_interface_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let token = db::regenerate_interface_token(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("接口 {} 不存在", id)))?;
    Ok(Json(serde_json::json!({ "token": token })))
}

async fn create_interface_model(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateInterfaceModelRequest>,
) -> Result<impl IntoResponse, AppError> {
    let normalized = normalize_interface_models(&[InterfaceModelInput {
        provider_id: req.provider_id,
        upstream_model: req.upstream_model,
        model_name: req.model_name,
    }])?;
    let input = normalized
        .first()
        .expect("single interface model input must remain present");
    let model_name = input
        .model_name
        .as_deref()
        .expect("normalized interface model name must be present");
    let model = db::create_interface_model(
        &state.db,
        &id,
        model_name,
        &input.provider_id,
        &input.upstream_model,
    )
    .await
    .map_err(|error| map_interface_write_error(error, Some(&id)))?;
    Ok((
        StatusCode::CREATED,
        Json(crate::models::InterfaceModelResponse::from(model)),
    ))
}

async fn delete_interface_model(
    State(state): State<AppState>,
    Path((interface_id, model_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let deleted = db::delete_interface_model(&state.db, &interface_id, &model_id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("接口模型 {} 不存在", model_id)));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn normalize_interface_models(
    models: &[InterfaceModelInput],
) -> Result<Vec<InterfaceModelInput>, AppError> {
    if models.is_empty() {
        return Err(AppError::BadRequest("接口至少需要配置一个模型".to_string()));
    }
    let mut model_names = HashSet::with_capacity(models.len());
    models
        .iter()
        .map(|model| {
            let provider_id = model.provider_id.trim();
            if provider_id.is_empty() {
                return Err(AppError::BadRequest("Provider 不能为空".to_string()));
            }
            let upstream_model = model.upstream_model.trim();
            if upstream_model.is_empty() {
                return Err(AppError::BadRequest("上游模型不能为空".to_string()));
            }
            let model_name = model
                .model_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(upstream_model);
            if !model_names.insert(model_name.to_string()) {
                return Err(AppError::BadRequest(format!(
                    "接口模型名称 {} 重复",
                    model_name
                )));
            }
            Ok(InterfaceModelInput {
                provider_id: provider_id.to_string(),
                upstream_model: upstream_model.to_string(),
                model_name: Some(model_name.to_string()),
            })
        })
        .collect()
}

fn map_interface_write_error(
    error: db::InterfaceWriteError,
    interface_id: Option<&str>,
) -> AppError {
    match error {
        db::InterfaceWriteError::InterfaceNotFound => {
            AppError::NotFound(format!("接口 {} 不存在", interface_id.unwrap_or_default()))
        }
        db::InterfaceWriteError::ProviderModelNotFound {
            provider_id,
            upstream_model,
        } => AppError::BadRequest(format!(
            "Provider {} 未配置上游模型 {}",
            provider_id, upstream_model
        )),
        db::InterfaceWriteError::DuplicateModelName { model_name } => {
            AppError::BadRequest(format!("接口模型名称 {} 重复", model_name))
        }
        db::InterfaceWriteError::Storage(error) => AppError::Internal(error),
    }
}

fn validate_interface_name(name: &str) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(AppError::BadRequest("接口名称不能为空".to_string()));
    }
    Ok(())
}

async fn create_model_alias(
    State(state): State<AppState>,
    Json(req): Json<CreateModelAliasRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.alias.trim().is_empty() {
        return Err(AppError::BadRequest("别名不能为空".to_string()));
    }
    if req.provider_id.trim().is_empty() {
        return Err(AppError::BadRequest("Provider 不能为空".to_string()));
    }
    if req.upstream_model.trim().is_empty() {
        return Err(AppError::BadRequest("上游模型不能为空".to_string()));
    }
    if req.downstream_protocols.is_empty() {
        return Err(AppError::BadRequest("下游协议不能为空".to_string()));
    }
    let provider = db::get_config_by_id(&state.db, &req.provider_id)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("Provider {} 不存在", req.provider_id)))?;
    let provider_spec = ProviderSpec::from_provider_config(&provider);
    if let Some(protocol) = req
        .downstream_protocols
        .iter()
        .find(|protocol| !provider_spec.supports_downstream(protocol))
    {
        return Err(AppError::BadRequest(format!(
            "Provider 上游协议不支持下游协议 {protocol}"
        )));
    }
    let protocols = req
        .downstream_protocols
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let alias = db::create_model_alias(
        &state.db,
        &req.alias,
        &req.provider_id,
        &req.upstream_model,
        &protocols,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(ModelAliasResponse::from(alias))))
}

async fn delete_model_alias_protocol(
    State(state): State<AppState>,
    Query(req): Query<DeleteModelAliasProtocolQuery>,
) -> Result<impl IntoResponse, AppError> {
    if req.alias.trim().is_empty() {
        return Err(AppError::BadRequest("别名不能为空".to_string()));
    }
    if req.downstream_protocol.trim().is_empty() {
        return Err(AppError::BadRequest("下游协议不能为空".to_string()));
    }

    let deleted =
        db::delete_model_alias_protocol(&state.db, &req.alias, &req.downstream_protocol).await?;
    if !deleted {
        return Err(AppError::NotFound(format!(
            "模型别名 {} 不存在或未启用协议 {}",
            req.alias, req.downstream_protocol
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::{http::HeaderMap, Router};
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    use crate::{db, models::InterfaceModelInput, AppState};

    #[tokio::test]
    async fn creates_config_with_complete_trimmed_model_set() {
        let (addr, server, state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/configs"))
            .json(&json!({
                "name": "DeepSeek",
                "provider_type": "openai_compatible",
                "base_url": "https://api.deepseek.com/v1",
                "api_key": "sk-upstream",
                "models": [" deepseek-chat ", "deepseek-reasoner"]
            }))
            .send()
            .await
            .expect("send create config request");

        assert_eq!(response.status(), reqwest::StatusCode::CREATED);
        let body: serde_json::Value = response.json().await.expect("parse config json");
        assert_eq!(body["models"][0]["model_name"], "deepseek-chat");
        assert_eq!(body["models"][1]["model_name"], "deepseek-reasoner");

        let stored = db::list_provider_models_by_provider(
            &state.db,
            body["id"].as_str().expect("provider id"),
        )
        .await
        .expect("list provider models");
        assert_eq!(stored.len(), 2);

        server.abort();
    }

    #[tokio::test]
    async fn rejects_config_creation_without_models_without_persisting_provider() {
        let (addr, server, state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/configs"))
            .json(&json!({
                "name": "DeepSeek",
                "provider_type": "openai_compatible",
                "base_url": "https://api.deepseek.com/v1",
                "api_key": "sk-upstream"
            }))
            .send()
            .await
            .expect("send create config request without models");

        assert!(response.status().is_client_error());
        let stored = db::list_configs(&state.db)
            .await
            .expect("list provider configs");
        assert!(stored.is_empty());

        server.abort();
    }

    #[tokio::test]
    async fn updates_config_with_authoritative_trimmed_model_set() {
        let (addr, server, _state) = spawn_admin_app().await;
        let client = reqwest::Client::new();
        let create_response = client
            .post(format!("http://{addr}/api/configs"))
            .json(&json!({
                "name": "Original Provider",
                "provider_type": "openai_compatible",
                "base_url": "https://old.example",
                "api_key": "sk-old",
                "models": ["model-a", "model-b"]
            }))
            .send()
            .await
            .expect("send create config request");
        let created: serde_json::Value =
            create_response.json().await.expect("parse created config");

        let response = client
            .put(format!(
                "http://{addr}/api/configs/{}",
                created["id"].as_str().expect("provider id")
            ))
            .json(&json!({
                "name": "Updated Provider",
                "models": [" model-b ", "model-c"]
            }))
            .send()
            .await
            .expect("send update config request");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = response.json().await.expect("parse updated config");
        assert_eq!(body["name"], "Updated Provider");
        let model_names = body["models"]
            .as_array()
            .expect("models array")
            .iter()
            .map(|model| model["model_name"].as_str().expect("model name"))
            .collect::<Vec<_>>();
        assert_eq!(model_names.len(), 2);
        assert!(model_names.contains(&"model-b"));
        assert!(model_names.contains(&"model-c"));

        server.abort();
    }

    #[tokio::test]
    async fn updating_config_without_models_preserves_models_in_response() {
        let (addr, server, _state) = spawn_admin_app().await;
        let client = reqwest::Client::new();
        let create_response = client
            .post(format!("http://{addr}/api/configs"))
            .json(&json!({
                "name": "Original Provider",
                "provider_type": "openai_compatible",
                "base_url": "https://old.example",
                "api_key": "sk-old",
                "models": ["model-a", "model-b"]
            }))
            .send()
            .await
            .expect("send create config request");
        let created: serde_json::Value =
            create_response.json().await.expect("parse created config");

        let response = client
            .put(format!(
                "http://{addr}/api/configs/{}",
                created["id"].as_str().expect("provider id")
            ))
            .json(&json!({ "name": "Updated Provider" }))
            .send()
            .await
            .expect("send update config request");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = response.json().await.expect("parse updated config");
        assert_eq!(body["models"].as_array().expect("models array").len(), 2);

        server.abort();
    }

    #[tokio::test]
    async fn deletes_provider_and_all_associations_from_admin_api() {
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        db::create_provider_model(&state.db, &provider.id, "model-a")
            .await
            .expect("create provider model");
        let interface = db::create_interface(&state.db, "Interface", "responses")
            .await
            .expect("create interface");
        db::create_interface_model(&state.db, &interface.id, "model-a", &provider.id, "model-a")
            .await
            .expect("create interface model");
        db::create_model_alias(
            &state.db,
            "model-alias",
            &provider.id,
            "model-a",
            &["responses"],
        )
        .await
        .expect("create model alias");

        let response = reqwest::Client::new()
            .delete(format!("http://{addr}/api/configs/{}", provider.id))
            .send()
            .await
            .expect("send delete config request");

        assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
        assert!(db::get_config_by_id(&state.db, &provider.id)
            .await
            .expect("get deleted provider")
            .is_none());
        for table in ["provider_models", "interface_models", "model_aliases"] {
            let count = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM {table} WHERE provider_id = ?"
            ))
            .bind(&provider.id)
            .fetch_one(&state.db)
            .await
            .expect("count provider associations");
            assert_eq!(count, 0, "{table} rows should be deleted");
        }

        server.abort();
    }

    #[tokio::test]
    async fn returns_not_found_without_deleting_orphaned_associations() {
        let (addr, server, state) = spawn_admin_app().await;
        let missing_provider_id = "missing-provider";
        sqlx::query(
            "INSERT INTO provider_models (id, provider_id, model_name, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind("orphaned-provider-model")
        .bind(missing_provider_id)
        .bind("model-a")
        .bind("2026-08-01T00:00:00Z")
        .execute(&state.db)
        .await
        .expect("insert orphaned provider model");
        sqlx::query(
            "INSERT INTO interface_models (id, interface_id, model_name, provider_id, upstream_model, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("orphaned-interface-model")
        .bind("missing-interface")
        .bind("model-a")
        .bind(missing_provider_id)
        .bind("model-a")
        .bind("2026-08-01T00:00:00Z")
        .execute(&state.db)
        .await
        .expect("insert orphaned interface model");
        sqlx::query(
            "INSERT INTO model_aliases (id, alias, provider_id, upstream_model, downstream_protocols_json, enabled, created_at) VALUES (?, ?, ?, ?, ?, 1, ?)",
        )
        .bind("orphaned-alias-id")
        .bind("orphaned-alias")
        .bind(missing_provider_id)
        .bind("model-a")
        .bind("[\"responses\"]")
        .bind("2026-08-01T00:00:00Z")
        .execute(&state.db)
        .await
        .expect("insert orphaned model alias");

        let response = reqwest::Client::new()
            .delete(format!("http://{addr}/api/configs/{missing_provider_id}"))
            .send()
            .await
            .expect("send delete missing config request");

        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        for table in ["provider_models", "interface_models", "model_aliases"] {
            let count = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM {table} WHERE provider_id = ?"
            ))
            .bind(missing_provider_id)
            .fetch_one(&state.db)
            .await
            .expect("count orphaned associations");
            assert_eq!(count, 1, "{table} row should remain");
        }

        server.abort();
    }

    #[tokio::test]
    async fn rejects_empty_and_duplicate_models_in_config_routes() {
        let (addr, server, state) = spawn_admin_app().await;
        let client = reqwest::Client::new();

        let empty_response = client
            .post(format!("http://{addr}/api/configs"))
            .json(&json!({
                "name": "Invalid Provider",
                "provider_type": "openai_compatible",
                "base_url": "https://invalid.example",
                "api_key": "sk-invalid",
                "models": ["model-a", "   "]
            }))
            .send()
            .await
            .expect("send empty model request");
        assert_eq!(empty_response.status(), reqwest::StatusCode::BAD_REQUEST);

        let provider = db::create_config(
            &state.db,
            "Existing Provider",
            "openai_compatible",
            "https://existing.example",
            "sk-existing",
        )
        .await
        .expect("create provider");
        let duplicate_response = client
            .put(format!("http://{addr}/api/configs/{}", provider.id))
            .json(&json!({ "models": [" model-a", "model-a "] }))
            .send()
            .await
            .expect("send duplicate model request");
        assert_eq!(
            duplicate_response.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

        server.abort();
    }

    #[tokio::test]
    async fn creates_config_with_capability_overrides() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/configs"))
            .json(&json!({
                "name": "DeepSeek",
                "provider_type": "openai_compatible",
                "base_url": "https://api.deepseek.com/v1",
                "api_key": "sk-upstream",
                "models": [],
                "capabilities": {
                    "upstream_protocols": ["anthropic"],
                    "tool_calls": true,
                    "structured_outputs": true,
                    "max_context_tokens": 8192,
                    "max_output_tokens": 2048
                }
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::CREATED);

        let body: serde_json::Value = response.json().await.expect("parse config json");
        assert_eq!(
            body["capabilities"]["upstream_protocols"],
            json!(["anthropic"])
        );
        assert_eq!(body["capabilities"]["tool_calls"], true);
        assert_eq!(body["capabilities"]["structured_outputs"], true);
        assert_eq!(body["capabilities"]["max_context_tokens"], 8192);
        assert_eq!(body["capabilities"]["max_output_tokens"], 2048);

        server.abort();
    }

    #[tokio::test]
    async fn creates_config_with_protocol_base_url_overrides() {
        let (addr, server, _state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/configs"))
            .json(&json!({
                "name": "DeepSeek",
                "provider_type": "openai_compatible",
                "base_url": "https://default.example",
                "api_key": "sk-test",
                "models": [],
                "capabilities": {
                    "upstream_protocols": ["openai", "anthropic"],
                    "protocol_base_urls": {
                        "openai": "https://chat.example/v1",
                        "anthropic": ""
                    }
                }
            }))
            .send()
            .await
            .expect("send create config request");

        assert_eq!(response.status(), reqwest::StatusCode::CREATED);
        let body: serde_json::Value = response.json().await.expect("parse config json");
        assert_eq!(
            body["capabilities"]["protocol_base_urls"]["openai"],
            "https://chat.example/v1"
        );
        assert_eq!(body["capabilities"]["protocol_base_urls"]["anthropic"], "");

        server.abort();
    }

    #[tokio::test]
    async fn creates_model_alias_from_admin_api() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/model-aliases"))
            .json(&json!({
                "alias": "coder",
                "provider_id": provider.id,
                "upstream_model": "deepseek-chat",
                "downstream_protocols": ["responses", "chat_completions"]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::CREATED);

        let resolved = db::get_provider_by_model(&state.db, "coder", "responses")
            .await
            .expect("resolve alias")
            .expect("alias exists");
        assert_eq!(resolved.model_upstream, "deepseek-chat");

        server.abort();
    }

    #[tokio::test]
    async fn manages_provider_models_from_config_admin_api() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });
        let client = reqwest::Client::new();

        let response = client
            .post(format!("http://{addr}/api/configs/{}/models", provider.id))
            .json(&json!({ "model_name": "deepseek-chat" }))
            .send()
            .await
            .expect("send create provider model request");
        assert_eq!(response.status(), reqwest::StatusCode::CREATED);
        let created: serde_json::Value = response.json().await.expect("parse model json");
        assert_eq!(created["provider_id"], provider.id);
        assert_eq!(created["model_name"], "deepseek-chat");

        let response = client
            .post(format!("http://{addr}/api/configs/{}/models", provider.id))
            .json(&json!({ "model_name": "deepseek-chat" }))
            .send()
            .await
            .expect("send duplicate provider model request");
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let error: serde_json::Value = response.json().await.expect("parse error json");
        assert_eq!(error["error"], "模型 deepseek-chat 已存在");

        let response = client
            .get(format!("http://{addr}/api/configs/{}/models", provider.id))
            .send()
            .await
            .expect("send list provider models request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let models: serde_json::Value = response.json().await.expect("parse models json");
        assert_eq!(models[0]["model_name"], "deepseek-chat");

        let response = client
            .get(format!("http://{addr}/api/configs"))
            .send()
            .await
            .expect("send list configs request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let configs: serde_json::Value = response.json().await.expect("parse configs json");
        assert_eq!(configs[0]["models"][0]["model_name"], "deepseek-chat");

        let response = client
            .delete(format!(
                "http://{addr}/api/configs/{}/models/{}",
                provider.id,
                created["id"].as_str().expect("model id")
            ))
            .send()
            .await
            .expect("send delete provider model request");
        assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

        let response = client
            .get(format!("http://{addr}/api/configs/{}/models", provider.id))
            .send()
            .await
            .expect("send list provider models request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let models: serde_json::Value = response.json().await.expect("parse models json");
        assert_eq!(models.as_array().expect("models array").len(), 0);

        server.abort();
    }

    #[tokio::test]
    async fn discovers_models_for_unsaved_bearer_provider_from_admin_api() {
        let (upstream_addr, upstream_server) =
            spawn_models_upstream(ExpectedAuth::Bearer, "sk-form-secret").await;
        let (addr, server, _state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/configs/discover-models"))
            .json(&json!({
                "provider_type": "openai_compatible",
                "base_url": format!("http://{upstream_addr}/"),
                "api_key": "sk-form-secret"
            }))
            .send()
            .await
            .expect("send discover models request");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = response.json().await.expect("parse models json");
        assert_eq!(
            body["models"],
            json!(["deepseek-chat", "deepseek-reasoner"])
        );

        server.abort();
        upstream_server.abort();
    }

    #[tokio::test]
    async fn discovers_and_syncs_saved_anthropic_provider_models_without_duplicates() {
        let (upstream_addr, upstream_server) =
            spawn_models_upstream(ExpectedAuth::Anthropic, "sk-saved-secret").await;
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Anthropic Provider",
            "anthropic_compatible",
            &format!("http://{upstream_addr}"),
            "sk-saved-secret",
        )
        .await
        .expect("create provider");

        for _ in 0..2 {
            let response = reqwest::Client::new()
                .post(format!(
                    "http://{addr}/api/configs/{}/discover-models",
                    provider.id
                ))
                .send()
                .await
                .expect("send saved discover models request");

            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let body: serde_json::Value = response.json().await.expect("parse models json");
            assert_eq!(
                body["models"],
                json!(["claude-opus-4-20250514", "claude-sonnet-4-20250514"])
            );
        }

        let stored = db::list_provider_models_by_provider(&state.db, &provider.id)
            .await
            .expect("list provider models");
        assert_eq!(stored.len(), 2);
        assert!(stored
            .iter()
            .any(|model| model.model_name == "claude-sonnet-4-20250514"));
        assert!(stored
            .iter()
            .any(|model| model.model_name == "claude-opus-4-20250514"));

        server.abort();
        upstream_server.abort();
    }

    #[tokio::test]
    async fn discover_models_reports_upstream_status_without_leaking_api_key() {
        let app = Router::new().route(
            "/models",
            axum::routing::get(|| async {
                (
                    reqwest::StatusCode::BAD_GATEWAY,
                    axum::Json(json!({ "error": "bad upstream" })),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream server");
        let upstream_addr = listener.local_addr().expect("read upstream server address");
        let upstream_server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        let (addr, server, _state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/configs/discover-models"))
            .json(&json!({
                "provider_type": "openai_compatible",
                "base_url": format!("http://{upstream_addr}"),
                "api_key": "sk-do-not-leak"
            }))
            .send()
            .await
            .expect("send discover models request");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let body: serde_json::Value = response.json().await.expect("parse error json");
        let error = body["error"].as_str().expect("error message");
        assert!(error.contains("模型获取失败"));
        assert!(error.contains("502"));
        assert!(!error.contains("sk-do-not-leak"));

        server.abort();
        upstream_server.abort();
    }

    #[tokio::test]
    async fn tests_unsaved_chat_protocol_latency_from_admin_api() {
        let (upstream_addr, upstream_server) = spawn_chat_test_upstream("sk-test-secret").await;
        let (addr, server, _state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/configs/test-protocol"))
            .json(&json!({
                "provider_type": "openai_compatible",
                "protocol": "openai",
                "base_url": format!("http://{upstream_addr}"),
                "api_key": "sk-test-secret",
                "model": "deepseek-chat"
            }))
            .send()
            .await
            .expect("send test protocol request");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = response.json().await.expect("parse test json");
        assert_eq!(body["ok"], true);
        assert_eq!(body["protocol"], "openai");
        assert!(body["latency_ms"].as_i64().expect("latency") >= 0);

        server.abort();
        upstream_server.abort();
    }

    #[tokio::test]
    async fn pings_saved_provider_without_generating_or_syncing_models() {
        let (upstream_addr, upstream_server) =
            spawn_models_upstream(ExpectedAuth::Bearer, "sk-ping-secret").await;
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "DeepSeek",
            "openai_compatible",
            &format!("http://{upstream_addr}"),
            "sk-ping-secret",
        )
        .await
        .expect("create provider");

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/configs/{}/ping", provider.id))
            .send()
            .await
            .expect("send provider ping request");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = response.json().await.expect("parse ping json");
        assert_eq!(body["ok"], true);
        assert!(body["latency_ms"].as_i64().expect("latency") >= 0);

        let stored = db::list_provider_models_by_provider(&state.db, &provider.id)
            .await
            .expect("list provider models");
        assert!(stored.is_empty());

        server.abort();
        upstream_server.abort();
    }

    #[tokio::test]
    async fn rejects_interface_model_missing_from_provider_catalog_from_admin_api() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let interface = db::create_interface(&db, "Responses", "responses")
            .await
            .expect("create interface");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!(
                "http://{addr}/api/interfaces/{}/models",
                interface.id
            ))
            .json(&json!({
                "provider_id": provider.id,
                "upstream_model": "not-in-catalog",
                "model_name": "coder"
            }))
            .send()
            .await
            .expect("send create interface model request");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let body: serde_json::Value = response.json().await.expect("parse error body");
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("未配置上游模型"));

        server.abort();
    }

    #[tokio::test]
    async fn creates_interface_with_complete_trimmed_model_mapping() {
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        db::create_provider_model(&state.db, &provider.id, "model-a")
            .await
            .expect("create provider model");

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/interfaces"))
            .json(&json!({
                "name": "Main",
                "models": [{
                    "provider_id": format!(" {} ", provider.id),
                    "upstream_model": " model-a ",
                    "model_name": "   "
                }]
            }))
            .send()
            .await
            .expect("send create interface request");

        assert_eq!(response.status(), reqwest::StatusCode::CREATED);
        let body: serde_json::Value = response.json().await.expect("parse interface json");
        assert_eq!(body["name"], "Main");
        assert_eq!(body["protocol"], "all");
        assert_eq!(body["models"][0]["provider_id"], provider.id);
        assert_eq!(body["models"][0]["upstream_model"], "model-a");
        assert_eq!(body["models"][0]["model_name"], "model-a");

        server.abort();
    }

    #[tokio::test]
    async fn rejects_interface_creation_with_empty_models() {
        let (addr, server, state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/interfaces"))
            .json(&json!({ "name": "Main", "models": [] }))
            .send()
            .await
            .expect("send create interface request");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert!(db::list_interfaces(&state.db)
            .await
            .expect("list interfaces")
            .is_empty());

        server.abort();
    }

    #[tokio::test]
    async fn rejects_interface_creation_without_models_field() {
        let (addr, server, state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/interfaces"))
            .json(&json!({ "name": "Main" }))
            .send()
            .await
            .expect("send create interface request");

        assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
        assert!(db::list_interfaces(&state.db)
            .await
            .expect("list interfaces")
            .is_empty());

        server.abort();
    }

    #[tokio::test]
    async fn rejects_invalid_interface_mapping_with_bad_request_and_rolls_back_creation() {
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/interfaces"))
            .json(&json!({
                "name": "Must Roll Back",
                "models": [{
                    "provider_id": provider.id,
                    "upstream_model": "missing-model"
                }]
            }))
            .send()
            .await
            .expect("send create interface request");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert!(db::list_interfaces(&state.db)
            .await
            .expect("list interfaces")
            .is_empty());

        server.abort();
    }

    #[tokio::test]
    async fn rejects_missing_provider_in_interface_mapping_with_bad_request() {
        let (addr, server, state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/interfaces"))
            .json(&json!({
                "name": "Missing Provider",
                "models": [{
                    "provider_id": "missing-provider",
                    "upstream_model": "model-a"
                }]
            }))
            .send()
            .await
            .expect("send create interface request");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert!(db::list_interfaces(&state.db)
            .await
            .expect("list interfaces")
            .is_empty());

        server.abort();
    }

    #[tokio::test]
    async fn rejects_duplicate_normalized_interface_model_names() {
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        for model_name in ["model-a", "model-b"] {
            db::create_provider_model(&state.db, &provider.id, model_name)
                .await
                .expect("create provider model");
        }

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/interfaces"))
            .json(&json!({
                "name": "Duplicate Aliases",
                "models": [
                    {
                        "provider_id": provider.id,
                        "upstream_model": "model-a",
                        "model_name": " shared "
                    },
                    {
                        "provider_id": provider.id,
                        "upstream_model": "model-b",
                        "model_name": "shared"
                    }
                ]
            }))
            .send()
            .await
            .expect("send create interface request");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert!(db::list_interfaces(&state.db)
            .await
            .expect("list interfaces")
            .is_empty());

        server.abort();
    }

    #[tokio::test]
    async fn update_interface_authoritatively_replaces_models_and_rolls_back_invalid_replacement() {
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        for model_name in ["model-a", "model-b"] {
            db::create_provider_model(&state.db, &provider.id, model_name)
                .await
                .expect("create provider model");
        }
        let client = reqwest::Client::new();
        let created: serde_json::Value = client
            .post(format!("http://{addr}/api/interfaces"))
            .json(&json!({
                "name": "Original",
                "models": [{
                    "provider_id": provider.id,
                    "upstream_model": "model-a",
                    "model_name": "old-alias"
                }]
            }))
            .send()
            .await
            .expect("send create interface request")
            .json()
            .await
            .expect("parse created interface");
        let interface_id = created["id"].as_str().expect("interface id");

        let replaced = client
            .put(format!("http://{addr}/api/interfaces/{interface_id}"))
            .json(&json!({
                "name": "Updated",
                "models": [{
                    "provider_id": provider.id,
                    "upstream_model": "model-b",
                    "model_name": "new-alias"
                }]
            }))
            .send()
            .await
            .expect("send interface update");

        assert_eq!(replaced.status(), reqwest::StatusCode::OK);
        let replaced_body: serde_json::Value =
            replaced.json().await.expect("parse updated interface");
        assert_eq!(replaced_body["models"].as_array().expect("models").len(), 1);
        assert_eq!(replaced_body["models"][0]["model_name"], "new-alias");

        let invalid = client
            .put(format!("http://{addr}/api/interfaces/{interface_id}"))
            .json(&json!({
                "name": "Must Roll Back",
                "models": [{
                    "provider_id": provider.id,
                    "upstream_model": "missing-model"
                }]
            }))
            .send()
            .await
            .expect("send invalid interface update");

        assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);
        let stored = db::get_interface_by_id(&state.db, interface_id)
            .await
            .expect("get interface")
            .expect("interface exists");
        assert_eq!(stored.name, "Updated");
        let models = db::list_interface_models_by_interface(&state.db, interface_id)
            .await
            .expect("list interface models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_name, "new-alias");

        server.abort();
    }

    #[tokio::test]
    async fn rejects_empty_interface_update_models_and_preserves_existing_mapping() {
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        db::create_provider_model(&state.db, &provider.id, "model-a")
            .await
            .expect("create provider model");
        let mappings = vec![InterfaceModelInput {
            provider_id: provider.id,
            upstream_model: "model-a".to_string(),
            model_name: Some("alias-a".to_string()),
        }];
        let (interface, original_models) =
            db::create_interface_with_models(&state.db, "Original", "all", &mappings)
                .await
                .expect("create interface");

        let response = reqwest::Client::new()
            .put(format!("http://{addr}/api/interfaces/{}", interface.id))
            .json(&json!({ "name": "Must Not Persist", "models": [] }))
            .send()
            .await
            .expect("send interface update");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let stored = db::get_interface_by_id(&state.db, &interface.id)
            .await
            .expect("get interface")
            .expect("interface exists");
        assert_eq!(stored.name, "Original");
        let models = db::list_interface_models_by_interface(&state.db, &interface.id)
            .await
            .expect("list interface models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, original_models[0].id);

        server.abort();
    }

    #[tokio::test]
    async fn updating_missing_interface_returns_not_found() {
        let (addr, server, _state) = spawn_admin_app().await;

        let response = reqwest::Client::new()
            .put(format!("http://{addr}/api/interfaces/missing-interface"))
            .json(&json!({ "name": "Updated" }))
            .send()
            .await
            .expect("send interface update");

        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

        server.abort();
    }

    #[tokio::test]
    async fn deleting_interface_model_through_wrong_parent_returns_not_found_and_preserves_model() {
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        db::create_provider_model(&state.db, &provider.id, "model-a")
            .await
            .expect("create provider model");
        let first = db::create_interface(&state.db, "First", "all")
            .await
            .expect("create first interface");
        let second = db::create_interface(&state.db, "Second", "all")
            .await
            .expect("create second interface");
        let model =
            db::create_interface_model(&state.db, &first.id, "alias-a", &provider.id, "model-a")
                .await
                .expect("create interface model");

        let response = reqwest::Client::new()
            .delete(format!(
                "http://{addr}/api/interfaces/{}/models/{}",
                second.id, model.id
            ))
            .send()
            .await
            .expect("send delete request");

        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        let models = db::list_interface_models_by_interface(&state.db, &first.id)
            .await
            .expect("list first interface models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, model.id);

        server.abort();
    }

    #[tokio::test]
    async fn creates_interface_model_without_filtering_by_legacy_interface_protocol() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "OpenAI",
            "openai",
            "https://api.openai.com/v1",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        db::create_provider_model(&db, &provider.id, "gpt-4.1")
            .await
            .expect("create provider model");
        let interface = db::create_interface(&db, "Legacy Chat", "chat_completions")
            .await
            .expect("create interface");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!(
                "http://{addr}/api/interfaces/{}/models",
                interface.id
            ))
            .json(&json!({
                "provider_id": provider.id,
                "upstream_model": "gpt-4.1",
                "model_name": "gpt"
            }))
            .send()
            .await
            .expect("send create interface model request");

        assert_eq!(response.status(), reqwest::StatusCode::CREATED);
        let body: serde_json::Value = response.json().await.expect("parse interface model json");
        assert_eq!(body["model_name"], "gpt");

        server.abort();
    }

    #[tokio::test]
    async fn rejects_duplicate_interface_model_name_without_adding_a_row() {
        let (addr, server, state) = spawn_admin_app().await;
        let provider = db::create_config(
            &state.db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        for model_name in ["model-a", "model-b"] {
            db::create_provider_model(&state.db, &provider.id, model_name)
                .await
                .expect("create provider model");
        }
        let interface = db::create_interface(&state.db, "Legacy", "all")
            .await
            .expect("create interface");
        db::create_interface_model(&state.db, &interface.id, "shared", &provider.id, "model-a")
            .await
            .expect("create existing interface model");

        let response = reqwest::Client::new()
            .post(format!(
                "http://{addr}/api/interfaces/{}/models",
                interface.id
            ))
            .json(&json!({
                "provider_id": provider.id,
                "upstream_model": "model-b",
                "model_name": " shared "
            }))
            .send()
            .await
            .expect("send duplicate interface model request");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let models = db::list_interface_models_by_interface(&state.db, &interface.id)
            .await
            .expect("list interface models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].upstream_model, "model-a");

        server.abort();
    }

    #[tokio::test]
    async fn rejects_model_alias_with_unsupported_downstream_protocols() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "OpenAI",
            "openai",
            "https://api.openai.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/model-aliases"))
            .json(&json!({
                "alias": "coder",
                "provider_id": provider.id,
                "upstream_model": "gpt-4.1",
                "downstream_protocols": ["responses", "chat_completions"]
            }))
            .send()
            .await
            .expect("send request");

        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let body: serde_json::Value = response.json().await.expect("parse error body");
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("chat_completions"));

        server.abort();
    }

    #[tokio::test]
    async fn lists_model_aliases_from_admin_api() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        db::create_model_alias(&db, "coder", &provider.id, "deepseek-chat", &["responses"])
            .await
            .expect("create alias");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .get(format!("http://{addr}/api/model-aliases"))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let aliases: serde_json::Value = response.json().await.expect("parse aliases json");
        assert_eq!(aliases[0]["alias"], "coder");
        assert_eq!(aliases[0]["provider_id"], provider.id);
        assert_eq!(aliases[0]["upstream_model"], "deepseek-chat");

        server.abort();
    }

    #[derive(Clone, Copy)]
    enum ExpectedAuth {
        Bearer,
        Anthropic,
    }

    async fn spawn_admin_app() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>, AppState) {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().nest("/api", super::router().with_state(state.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });
        (addr, server, state)
    }

    async fn spawn_models_upstream(
        expected_auth: ExpectedAuth,
        expected_key: &'static str,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/models",
            axum::routing::get(move |headers: HeaderMap| async move {
                let auth_is_valid = match expected_auth {
                    ExpectedAuth::Bearer => {
                        headers
                            .get(reqwest::header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            == Some(format!("Bearer {expected_key}").as_str())
                    }
                    ExpectedAuth::Anthropic => {
                        headers
                            .get("x-api-key")
                            .and_then(|value| value.to_str().ok())
                            == Some(expected_key)
                            && headers
                                .get("anthropic-version")
                                .and_then(|value| value.to_str().ok())
                                == Some("2023-06-01")
                    }
                };
                if !auth_is_valid {
                    return (
                        reqwest::StatusCode::UNAUTHORIZED,
                        axum::Json(json!({ "error": "bad auth" })),
                    );
                }

                let data = match expected_auth {
                    ExpectedAuth::Bearer => json!({
                        "data": [
                            { "id": "deepseek-reasoner" },
                            { "id": "" },
                            { "id": "deepseek-chat" },
                            { "id": "deepseek-chat" }
                        ]
                    }),
                    ExpectedAuth::Anthropic => json!({
                        "data": [
                            { "id": "claude-sonnet-4-20250514" },
                            { "id": "claude-opus-4-20250514" },
                            { "id": "claude-sonnet-4-20250514" }
                        ]
                    }),
                };
                (reqwest::StatusCode::OK, axum::Json(data))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream server");
        let addr = listener.local_addr().expect("read upstream server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        (addr, server)
    }

    async fn spawn_chat_test_upstream(
        expected_key: &'static str,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/chat/completions",
            axum::routing::post(
                move |headers: axum::http::HeaderMap,
                      axum::Json(payload): axum::Json<serde_json::Value>| async move {
                    let auth = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default();
                    assert_eq!(auth, format!("Bearer {expected_key}"));
                    assert_eq!(payload["model"], "deepseek-chat");
                    assert_eq!(payload["stream"], true);
                    axum::Json(json!({
                        "id": "chatcmpl_test",
                        "model": payload["model"],
                        "choices": [
                            {
                                "delta": {
                                    "content": "ok"
                                }
                            }
                        ]
                    }))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream server");
        let addr = listener.local_addr().expect("read upstream server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        (addr, server)
    }
}
