use axum::{
    body::Body,
    extract::{Extension, State},
    http::header,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use futures::TryStreamExt;
use serde_json::Value;

use crate::{
    error::AppError,
    observability::{
        request_metadata::build_request_metadata, stream_stats::record_first_chunk,
        upstream_observability::upstream_observability,
    },
    providers::spec::provider_upstream_base_url,
    routes::v1::auth::CurrentProtocolAccess,
    routes::v1::endpoint_resolver::{resolve_endpoint_model_candidates, ResolvedEndpointProvider},
    stats::RequestLogInsert,
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/chat/completions", post(create_chat_completion))
}

async fn create_chat_completion(
    State(state): State<AppState>,
    Extension(access): Extension<CurrentProtocolAccess>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("model 不能为空".to_string()))?
        .to_string();
    let is_streaming = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let candidates =
        resolve_endpoint_model_candidates(&state, &access, &model, "chat_completions").await?;
    let mut last_upstream_error = None;
    for resolved in candidates.into_iter().take(
        crate::upstream::policy()
            .max_candidates
            .unwrap_or(usize::MAX),
    ) {
        let provider_id = resolved.provider.id.clone();
        match crate::upstream::retry_with_policy(crate::upstream::policy(), || {
            create_chat_completion_with_candidate(
                &state,
                &access,
                payload.clone(),
                model.clone(),
                is_streaming,
                started_at,
                resolved.clone(),
            )
        })
        .await
        {
            Ok(response) => {
                if let Err(error) = state
                    .storage
                    .remember_protocol_model_provider(
                        &crate::storage::ProtocolAccess {
                            identity_id: access.identity_id.clone(),
                            endpoint_id: access.endpoint_id.clone(),
                            endpoint_name: access.endpoint_name.clone(),
                        },
                        &model,
                        &provider_id,
                    )
                    .await
                {
                    tracing::warn!(error = %error, "failed to remember active endpoint model provider");
                }
                return Ok(response);
            }
            Err(error) if error.is_retryable_upstream() => last_upstream_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_upstream_error
        .unwrap_or_else(|| AppError::BadRequest(format!("接入点未配置模型 {model}"))))
}

async fn create_chat_completion_with_candidate(
    state: &AppState,
    access: &CurrentProtocolAccess,
    mut payload: Value,
    model: String,
    is_streaming: bool,
    started_at: std::time::Instant,
    resolved: ResolvedEndpointProvider,
) -> Result<Response, AppError> {
    let provider = resolved.provider;
    let model_upstream = resolved.model_upstream;
    let metadata_json = build_request_metadata(Vec::new())?;

    payload["model"] = Value::String(model_upstream.clone());
    let upstream_base_url = provider_upstream_base_url(&provider, resolved.upstream_protocol);
    let upstream_url = format!(
        "{}/chat/completions",
        upstream_base_url.trim_end_matches('/')
    );
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::Upstream {
            status: None,
            message: format!("上游连接失败: {error}"),
        })?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        let observability_headers = upstream_response.headers().clone();
        let error_body = upstream_response.text().await.ok();
        let observability = upstream_observability(&observability_headers, error_body.as_deref());
        state
            .storage
            .insert_request_log(
                &access.identity_id,
                RequestLogInsert {
                    protocol_in: "chat_completions".to_string(),
                    protocol_out: "chat_completions".to_string(),
                    protocol_upstream: "chat_completions".to_string(),
                    provider_id: provider.id,
                    provider_name: provider.name,
                    endpoint_name: access.endpoint_name.clone(),
                    model_requested: model.clone(),
                    model_upstream,
                    status: "failed".to_string(),
                    http_status: status.as_u16() as i64,
                    error_code: None,
                    error_message: observability.error_message,
                    is_streaming,
                    input_tokens: None,
                    output_tokens: None,
                    reasoning_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    latency_ms: started_at.elapsed().as_millis() as i64,
                    upstream_latency_ms: None,
                    first_token_ms: None,
                    tool_call_count: None,
                    upstream_request_id: observability.request_id,
                    metadata_json: metadata_json.clone(),
                },
            )
            .await?;
        return Err(AppError::Upstream {
            status: Some(status),
            message: format!("上游请求失败: {status}"),
        });
    }

    if is_streaming {
        let upstream_request_id =
            upstream_observability(upstream_response.headers(), None).request_id;
        let log = RequestLogInsert {
            protocol_in: "chat_completions".to_string(),
            protocol_out: "chat_completions".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            endpoint_name: access.endpoint_name.clone(),
            model_requested: model,
            model_upstream,
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id,
            metadata_json: metadata_json.clone(),
        };
        let body = Body::from_stream(record_first_chunk(
            state.storage.clone(),
            access.identity_id.clone(),
            upstream_response
                .bytes_stream()
                .map_err(std::io::Error::other),
            log,
            started_at,
        ));
        return Ok(([(header::CONTENT_TYPE, "text/event-stream")], body).into_response());
    }

    let upstream_request_id = upstream_observability(upstream_response.headers(), None).request_id;
    let response = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    state
        .storage
        .insert_request_log(
            &access.identity_id,
            RequestLogInsert {
                protocol_in: "chat_completions".to_string(),
                protocol_out: "chat_completions".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                endpoint_name: access.endpoint_name.clone(),
                model_requested: model,
                model_upstream: response
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                status: "success".to_string(),
                http_status: 200,
                error_code: None,
                error_message: None,
                is_streaming,
                input_tokens: response
                    .get("usage")
                    .and_then(|usage| usage.get("prompt_tokens"))
                    .and_then(Value::as_i64),
                output_tokens: response
                    .get("usage")
                    .and_then(|usage| usage.get("completion_tokens"))
                    .and_then(Value::as_i64),
                reasoning_tokens: response
                    .get("usage")
                    .and_then(|usage| usage.get("completion_tokens_details"))
                    .and_then(|details| details.get("reasoning_tokens"))
                    .and_then(Value::as_i64),
                cache_read_tokens: response
                    .get("usage")
                    .and_then(|usage| {
                        usage
                            .pointer("/prompt_tokens_details/cached_tokens")
                            .or_else(|| usage.get("cache_read_input_tokens"))
                    })
                    .and_then(Value::as_i64),
                cache_write_tokens: response
                    .get("usage")
                    .and_then(|usage| usage.get("cache_creation_input_tokens"))
                    .and_then(Value::as_i64),
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: Some(upstream_latency_ms),
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id,
                metadata_json,
            },
        )
        .await?;

    Ok(Json(response).into_response())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::State,
        http::{Request, StatusCode},
        middleware,
        response::IntoResponse,
        routing::post,
        Json, Router,
    };
    use bytes::Bytes;
    use futures::StreamExt;
    use serde_json::json;
    use std::{convert::Infallible, time::Duration};
    use tokio::net::TcpListener;
    use tower::ServiceExt;

    use super::create_chat_completion;
    use crate::{
        models::ProviderCapabilityOverrides,
        routes::v1::endpoint_resolver::{
            create_test_endpoint_auth, create_test_endpoint_auth_with_candidates, test_provider,
            test_provider_with_capabilities,
        },
        AppState,
    };

    async fn test_state() -> AppState {
        crate::test_support::test_state().await
    }

    #[tokio::test]
    async fn rejects_unauthenticated_chat_completion_request() {
        let state = test_state().await;
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state,
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("route request");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn forwards_chat_completion_request_to_configured_upstream() {
        let upstream = spawn_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let response = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");

        let response = response_json(response).await;

        assert_eq!(response["id"], "chatcmpl_test");
        assert_eq!(
            response["choices"][0]["message"]["content"],
            "chat upstream hello"
        );
    }

    #[tokio::test]
    async fn fails_over_to_a_healthy_candidate_and_keeps_using_it() {
        let failing_upstream = spawn_failing_chat_upstream().await;
        let healthy_upstream = spawn_chat_upstream().await;
        let state = test_state().await;
        let primary = test_provider(
            "primary",
            "openai_compatible",
            &failing_upstream,
            "sk-primary",
        )
        .await
        .expect("create primary provider");
        let backup = test_provider(
            "backup",
            "openai_compatible",
            &healthy_upstream,
            "sk-backup",
        )
        .await
        .expect("create backup provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[primary, backup],
            "shared-model",
            "deepseek-chat",
        )
        .await;
        let identity_id = auth.access.0.identity_id.clone();
        let payload = json!({
            "model": "shared-model",
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let first = create_chat_completion(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(payload.clone()),
        )
        .await
        .expect("fall back to the healthy candidate");
        assert_eq!(response_json(first).await["id"], "chatcmpl_test");

        let second = create_chat_completion(State(state.clone()), auth.access, axum::Json(payload))
            .await
            .expect("keep using the last successful candidate");
        assert_eq!(response_json(second).await["id"], "chatcmpl_test");

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load request logs");
        assert_eq!(logs.len(), 3);
        assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
        assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
    }

    #[tokio::test]
    async fn resolves_endpoint_model_name_to_upstream_model() {
        let upstream = spawn_alias_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider(
            "DeepSeek Provider",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "coder", "deepseek-chat").await;

        let response = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "coder",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");

        let response = response_json(response).await;

        assert_eq!(response["model"], "deepseek-chat");
    }

    #[tokio::test]
    async fn rejects_chat_when_provider_only_supports_responses_upstream_protocol() {
        let state = test_state().await;
        let provider = test_provider(
            "DeepSeek Provider",
            "openai",
            "http://127.0.0.1:1",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "coder", "deepseek-chat").await;

        let error = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "coder",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect_err("unsupported provider protocol should fail before upstream");

        assert!(format!("{error:?}").contains("接入点未配置支持 chat_completions 的模型 coder"));
    }

    #[tokio::test]
    async fn rejects_chat_when_provider_only_supports_anthropic_messages_upstream_protocol() {
        let state = test_state().await;
        let provider = test_provider(
            "Claude",
            "anthropic_compatible",
            "http://127.0.0.1:1",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "Claude", "claude-sonnet-4").await;

        let error = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "Claude",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect_err("anthropic provider should not be exposed as chat completions");

        assert!(format!("{error:?}").contains("接入点未配置支持 chat_completions 的模型 Claude"));
    }

    #[tokio::test]
    async fn records_successful_chat_completion_request_log() {
        let upstream = spawn_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;
        let identity_id = auth.access.0.identity_id.clone();

        let _response = create_chat_completion(
            State(state.clone()),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");
        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load identity request log totals");

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, "success");
        assert_eq!(logs[0].input_tokens, Some(3));
        assert_eq!(logs[0].output_tokens, Some(4));
        assert_eq!(logs[0].endpoint_name.as_deref(), Some("Test Endpoint"));
    }

    #[tokio::test]
    async fn forwards_chat_completion_to_protocol_specific_base_url() {
        let default_upstream = spawn_unexpected_chat_upstream().await;
        let chat_upstream = spawn_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider_with_capabilities(
            "deepseek-chat",
            "openai_compatible",
            &default_upstream,
            "sk-upstream",
            Some(&ProviderCapabilityOverrides {
                protocol_base_urls: Some(crate::models::ProviderProtocolBaseUrls {
                    responses: None,
                    openai: Some(chat_upstream),
                    anthropic: None,
                    ..Default::default()
                }),
                ..ProviderCapabilityOverrides::default()
            }),
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let response = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");
        let body = response_json(response).await;

        assert_eq!(
            body["choices"][0]["message"]["content"],
            "chat upstream hello"
        );
    }

    #[tokio::test]
    async fn records_successful_chat_completion_upstream_request_id() {
        let upstream = spawn_request_id_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;
        let identity_id = auth.access.0.identity_id.clone();

        let _response = create_chat_completion(
            State(state.clone()),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");

        let upstream_request_id = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load upstream request id")[0]
            .upstream_request_id
            .clone();

        assert_eq!(upstream_request_id.as_deref(), Some("req_chat_123"));
    }

    #[tokio::test]
    async fn records_failed_chat_completion_upstream_request_id_and_error_message() {
        let upstream = spawn_error_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;
        let identity_id = auth.access.0.identity_id.clone();

        let error = create_chat_completion(
            State(state.clone()),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect_err("upstream error should fail");

        assert!(format!("{error:?}").contains("上游请求失败"));
        let row = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load failed request log");

        assert_eq!(row[0].upstream_request_id.as_deref(), Some("cf-ray-123"));
        assert_eq!(row[0].error_message.as_deref(), Some("provider overloaded"));
    }

    #[tokio::test]
    async fn streams_chat_completion_without_waiting_for_upstream_done() {
        let upstream = spawn_streaming_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider(
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;
        let identity_id = auth.access.0.identity_id.clone();
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let started = std::time::Instant::now();
        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/chat/completions"))
            .bearer_auth(&auth.token)
            .json(&json!({
                "model": "deepseek-chat",
                "stream": true,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .expect("stream response content type");
        assert!(
            content_type.starts_with("text/event-stream"),
            "unexpected content type: {content_type}"
        );

        let mut stream = response.bytes_stream();
        let first = stream
            .next()
            .await
            .expect("first response chunk")
            .expect("first response chunk ok");
        let elapsed = started.elapsed();
        let first = String::from_utf8(first.to_vec()).expect("first chunk utf8");

        assert!(
            elapsed < Duration::from_millis(200),
            "first chat completion chunk arrived after {elapsed:?}: {first}"
        );
        assert!(first.contains("data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}"));

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load stream request log");
        assert_eq!(logs.len(), 1);
        let first_token_ms = logs[0].first_token_ms;
        assert!(
            first_token_ms.is_some(),
            "first_token_ms should be recorded after the first stream chunk"
        );

        server.abort();
    }

    async fn spawn_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");
            Json(json!({
                "id": "chatcmpl_test",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "chat upstream hello"
                        }
                    }
                ],
                "usage": {
                    "prompt_tokens": 3,
                    "completion_tokens": 4
                }
            }))
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_failing_chat_upstream() -> String {
        async fn handler() -> axum::response::Response {
            use axum::{http::StatusCode, response::IntoResponse};

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "message": "primary unavailable" } })),
            )
                .into_response()
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_unexpected_chat_upstream() -> String {
        async fn handler() -> axum::response::Response {
            use axum::{http::StatusCode, response::IntoResponse};

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "default base url should not be used" })),
            )
                .into_response()
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_request_id_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            use axum::response::IntoResponse;

            assert_eq!(payload["model"], "deepseek-chat");
            (
                [("x-request-id", "req_chat_123")],
                Json(json!({
                    "id": "chatcmpl_request_id",
                    "model": payload["model"],
                    "choices": [
                        {
                            "message": {
                                "role": "assistant",
                                "content": "request id accepted"
                            }
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 1,
                        "completion_tokens": 2
                    }
                })),
            )
                .into_response()
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_error_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            use axum::{http::StatusCode, response::IntoResponse};

            assert_eq!(payload["model"], "deepseek-chat");
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("cf-ray", "cf-ray-123")],
                Json(json!({
                    "error": {
                        "message": "provider overloaded",
                        "type": "rate_limit_error"
                    }
                })),
            )
                .into_response()
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read response body");
        serde_json::from_slice(&body).expect("response body json")
    }

    async fn spawn_streaming_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["stream"], true);
            let stream = futures::stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                                  data: [DONE]\n\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream),
            )
                .into_response()
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_alias_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["messages"][0]["content"], "hello");
            Json(json!({
                "id": "chatcmpl_alias",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "alias accepted"
                        }
                    }
                ]
            }))
        }

        let app = Router::new().route("/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }
}
