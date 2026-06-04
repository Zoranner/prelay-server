use axum::{
    body::Body,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    bridge::{
        responses_decode::decode_responses_request,
        responses_encode::encode_responses_response,
        sessions::{load_response_session_messages, save_response_session},
        stream::chat_sse_response_to_responses_sse,
    },
    db,
    error::AppError,
    providers::chat_completions::{decode_chat_response, encode_chat_request},
    providers::ollama::{decode_ollama_chat_response, encode_ollama_chat_request},
    stats::{insert_request_log, RequestLogInsert},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/responses", post(create_response))
}

async fn create_response(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let original_payload = payload.clone();
    let request = decode_responses_request(payload)?;
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let previous_response_id = request.previous_response_id.clone();
    let resolved = db::get_provider_by_model(&state.db, &request.model)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("模型 {} 未配置", request.model)))?;
    let provider = resolved.provider;
    let model_upstream = resolved.model_upstream;
    let mut upstream_payload = original_payload.clone();
    upstream_payload["model"] = Value::String(model_upstream.clone());
    let mut request = request;
    request.model = model_upstream.clone();
    if provider.provider_type == "openai" {
        return create_native_response(
            &state,
            upstream_payload,
            provider,
            model_requested,
            is_streaming,
            started_at,
        )
        .await;
    }
    if provider.provider_type == "ollama_native" {
        return create_ollama_response(
            &state,
            request,
            provider,
            model_requested,
            is_streaming,
            previous_response_id,
            started_at,
        )
        .await;
    }

    let upstream_url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&encode_chat_request(
            &request_with_session_history(&state.db, request.clone()).await?,
        ))
        .send()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested,
                model_upstream: request.model,
                status: "failed".to_string(),
                http_status: status.as_u16() as i64,
                is_streaming,
                input_tokens: None,
                output_tokens: None,
                reasoning_tokens: None,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
    }

    if is_streaming {
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested,
                model_upstream: request.model,
                status: "success".to_string(),
                http_status: 200,
                is_streaming,
                input_tokens: None,
                output_tokens: None,
                reasoning_tokens: None,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
        )
        .await?;
        let body = Body::from_stream(chat_sse_response_to_responses_sse(upstream_response));
        return Ok((
            [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
            body,
        )
            .into_response());
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let mut response = decode_chat_response(upstream_json)?;
    response.id = format!("resp_{}", Uuid::new_v4().simple());
    let reasoning_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| usage.reasoning_tokens);
    let tool_call_count = count_tool_calls(&response);
    save_response_session(
        &state.db,
        &response.id,
        previous_response_id.as_deref(),
        &provider.id,
        &response.model,
        &request.messages,
        &response,
    )
    .await?;
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            model_requested,
            model_upstream: response.model.clone(),
            status: "success".to_string(),
            http_status: 200,
            is_streaming,
            input_tokens: response.usage.as_ref().and_then(|usage| usage.input_tokens),
            output_tokens: response
                .usage
                .as_ref()
                .and_then(|usage| usage.output_tokens),
            reasoning_tokens,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: Some(tool_call_count),
            upstream_request_id: None,
        },
    )
    .await?;

    Ok(Json(encode_responses_response(response)).into_response())
}

async fn create_ollama_response(
    state: &AppState,
    mut request: crate::bridge::internal::InternalRequest,
    provider: crate::models::ProviderConfig,
    model_requested: String,
    is_streaming: bool,
    previous_response_id: Option<String>,
    started_at: std::time::Instant,
) -> Result<Response, AppError> {
    if is_streaming {
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "ollama_native".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested,
                model_upstream: request.model,
                status: "failed".to_string(),
                http_status: 400,
                is_streaming,
                input_tokens: None,
                output_tokens: None,
                reasoning_tokens: None,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
        )
        .await?;
        return Err(AppError::BadRequest(
            "Ollama Responses 流式桥接暂未支持".to_string(),
        ));
    }

    request = request_with_session_history(&state.db, request).await?;
    let upstream_url = format!("{}/chat", provider.base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .json(&encode_ollama_chat_request(&request))
        .send()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "ollama_native".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested,
                model_upstream: request.model,
                status: "failed".to_string(),
                http_status: status.as_u16() as i64,
                is_streaming,
                input_tokens: None,
                output_tokens: None,
                reasoning_tokens: None,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let mut response = decode_ollama_chat_response(upstream_json)?;
    response.id = format!("resp_{}", Uuid::new_v4().simple());
    let tool_call_count = count_tool_calls(&response);
    save_response_session(
        &state.db,
        &response.id,
        previous_response_id.as_deref(),
        &provider.id,
        &response.model,
        &request.messages,
        &response,
    )
    .await?;
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "ollama_native".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            model_requested,
            model_upstream: response.model.clone(),
            status: "success".to_string(),
            http_status: 200,
            is_streaming,
            input_tokens: response.usage.as_ref().and_then(|usage| usage.input_tokens),
            output_tokens: response
                .usage
                .as_ref()
                .and_then(|usage| usage.output_tokens),
            reasoning_tokens: None,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: Some(tool_call_count),
            upstream_request_id: None,
        },
    )
    .await?;

    Ok(Json(encode_responses_response(response)).into_response())
}

async fn create_native_response(
    state: &AppState,
    payload: Value,
    provider: crate::models::ProviderConfig,
    model_requested: String,
    is_streaming: bool,
    started_at: std::time::Instant,
) -> Result<Response, AppError> {
    let upstream_url = format!("{}/responses", provider.base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "responses".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested,
                model_upstream: "unknown".to_string(),
                status: "failed".to_string(),
                http_status: status.as_u16() as i64,
                is_streaming,
                input_tokens: None,
                output_tokens: None,
                reasoning_tokens: None,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
    }

    let response = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "responses".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            model_requested,
            model_upstream: response
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            status: "success".to_string(),
            http_status: 200,
            is_streaming,
            input_tokens: response
                .get("usage")
                .and_then(|usage| usage.get("input_tokens"))
                .and_then(Value::as_i64),
            output_tokens: response
                .get("usage")
                .and_then(|usage| usage.get("output_tokens"))
                .and_then(Value::as_i64),
            reasoning_tokens: None,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
        },
    )
    .await?;

    Ok(Json(response).into_response())
}

async fn request_with_session_history(
    db: &sqlx::SqlitePool,
    mut request: crate::bridge::internal::InternalRequest,
) -> Result<crate::bridge::internal::InternalRequest, AppError> {
    let Some(previous_response_id) = request.previous_response_id.as_deref() else {
        return Ok(request);
    };
    let Some(mut history) = load_response_session_messages(db, previous_response_id).await? else {
        return Err(AppError::BadRequest(format!(
            "previous_response_id {previous_response_id} 不存在"
        )));
    };
    history.extend(request.messages);
    request.messages = history;
    Ok(request)
}

fn count_tool_calls(response: &crate::bridge::internal::InternalResponse) -> i64 {
    response
        .output
        .iter()
        .filter(|item| item.is_tool_call())
        .count() as i64
}

#[cfg(test)]
fn responses_sse_from_text_chunks(chunks: &[&str]) -> String {
    let mut output = String::new();
    for chunk in chunks {
        output.push_str(
            std::str::from_utf8(&crate::bridge::stream::responses_text_delta_sse(chunk))
                .expect("sse chunk is utf8"),
        );
    }
    output.push_str(
        std::str::from_utf8(&crate::bridge::stream::responses_completed_sse())
            .expect("sse chunk is utf8"),
    );
    output
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::{extract::State, middleware, response::IntoResponse, routing::post, Json, Router};
    use bytes::Bytes;
    use futures::StreamExt;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::{convert::Infallible, time::Duration};
    use tokio::net::TcpListener;

    use super::create_response;
    use super::responses_sse_from_text_chunks;
    use crate::{db, AppState};

    #[tokio::test]
    async fn rejects_unauthenticated_responses_request() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };
        let app = Router::new().merge(super::router().with_state(state.clone()).layer(
            middleware::from_fn_with_state(state, crate::routes::auth::require_protocol_auth),
        ));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/responses"))
            .json(&json!({
                "model": "deepseek-chat",
                "input": "hello"
            }))
            .send()
            .await
            .expect("send request");

        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        server.abort();
    }

    #[tokio::test]
    async fn rejects_response_when_model_is_not_configured() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let error = create_response(
            State(state),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect_err("missing provider should fail");

        assert!(format!("{error:?}").contains("模型 deepseek-chat 未配置"));
    }

    #[tokio::test]
    async fn forwards_responses_request_to_chat_completions_upstream() {
        let upstream = spawn_chat_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let response = create_response(
            State(state),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");

        let response = response_json(response).await;

        assert_eq!(response["object"], "response");
        assert_eq!(response["model"], "deepseek-chat");
        assert_eq!(
            response["output"][0]["content"][0]["text"],
            "upstream hello"
        );
        assert_eq!(response["usage"]["input_tokens"], 3);
        assert_eq!(response["usage"]["output_tokens"], 4);
    }

    #[tokio::test]
    async fn forwards_responses_request_to_native_upstream() {
        let upstream = spawn_native_responses_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(&db, "gpt-4.1", "openai", &upstream, "sk-upstream")
            .await
            .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let response = create_response(
            State(state),
            axum::Json(json!({
                "model": "gpt-4.1",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");
        let response = response_json(response).await;

        assert_eq!(response["id"], "resp_native");
        assert_eq!(response["object"], "response");
        assert_eq!(response["model"], "gpt-4.1");
        assert_eq!(
            response["output"][0]["content"][0]["text"],
            "native response"
        );
    }

    #[tokio::test]
    async fn forwards_responses_request_to_ollama_native_upstream() {
        let upstream = spawn_ollama_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(&db, "llama3.2", "ollama_native", &upstream, "unused")
            .await
            .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let response = create_response(
            State(state),
            axum::Json(json!({
                "model": "llama3.2",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");
        let response = response_json(response).await;

        assert_eq!(response["object"], "response");
        assert_eq!(response["model"], "llama3.2");
        assert_eq!(response["output"][0]["content"][0]["text"], "ollama hello");
        assert_eq!(response["usage"]["input_tokens"], 3);
        assert_eq!(response["usage"]["output_tokens"], 4);
    }

    #[tokio::test]
    async fn records_successful_ollama_responses_request_log() {
        let upstream = spawn_ollama_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(&db, "llama3.2", "ollama_native", &upstream, "unused")
            .await
            .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let _response = create_response(
            State(state.clone()),
            axum::Json(json!({
                "model": "llama3.2",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");

        let row: (String, Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT protocol_upstream, input_tokens, output_tokens FROM request_logs LIMIT 1",
        )
        .fetch_one(&state.db)
        .await
        .expect("load request log");

        assert_eq!(row.0, "ollama_native");
        assert_eq!(row.1, Some(3));
        assert_eq!(row.2, Some(4));
    }

    #[tokio::test]
    async fn records_successful_response_request_log() {
        let upstream = spawn_chat_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let _response = create_response(
            State(state.clone()),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect("create response");
        let overview = crate::stats::overview(&state.db)
            .await
            .expect("stats overview");

        assert_eq!(overview.total_requests, 1);
        assert_eq!(overview.successful_requests, 1);
        assert_eq!(overview.failed_requests, 0);
        assert_eq!(overview.input_tokens, 3);
        assert_eq!(overview.output_tokens, 4);
    }

    #[tokio::test]
    async fn records_failed_response_request_log_when_upstream_fails() {
        let upstream = spawn_failing_chat_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        create_response(
            State(state.clone()),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello"
            })),
        )
        .await
        .expect_err("upstream failure should fail");
        let overview = crate::stats::overview(&state.db)
            .await
            .expect("stats overview");

        assert_eq!(overview.total_requests, 1);
        assert_eq!(overview.successful_requests, 0);
        assert_eq!(overview.failed_requests, 1);
    }

    #[tokio::test]
    async fn returns_responses_sse_when_stream_is_true() {
        let upstream = spawn_streaming_chat_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let response = create_response(
            State(state),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "hello",
                "stream": true
            })),
        )
        .await
        .expect("create response")
        .into_response();
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read body");
        let body = String::from_utf8(body.to_vec()).expect("utf8 body");

        assert!(content_type.starts_with("text/event-stream"));
        assert!(body.contains("event: response.output_text.delta"));
        assert!(body.contains("data: hel"));
        assert!(body.contains("data: lo"));
        assert!(body.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn streams_responses_sse_delta_before_upstream_finishes() {
        let upstream = spawn_delayed_streaming_chat_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };
        let app = Router::new().merge(super::router().with_state(state));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let started = std::time::Instant::now();
        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/responses"))
            .json(&json!({
                "model": "deepseek-chat",
                "input": "hello",
                "stream": true
            }))
            .send()
            .await
            .expect("send request");
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
            "first relay chunk arrived after {elapsed:?}: {first}"
        );
        assert!(first.contains("event: response.output_text.delta"));
        assert!(first.contains("data: hel"));

        server.abort();
    }

    #[tokio::test]
    async fn prepends_previous_response_messages_to_upstream_chat_request() {
        let upstream = spawn_history_asserting_chat_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let first = create_response(
            State(state.clone()),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "first user"
            })),
        )
        .await
        .expect("create first response");
        let first = response_json(first).await;
        let first_id = first["id"].as_str().expect("first id");

        let second = create_response(
            State(state.clone()),
            axum::Json(json!({
                "model": "deepseek-chat",
                "previous_response_id": first_id,
                "input": "second user"
            })),
        )
        .await
        .expect("create second response");
        let second = response_json(second).await;

        assert_eq!(
            second["output"][0]["content"][0]["text"],
            "history accepted"
        );
    }

    #[tokio::test]
    async fn bridges_function_tool_call_roundtrip() {
        let upstream = spawn_tool_roundtrip_chat_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let first = create_response(
            State(state.clone()),
            axum::Json(json!({
                "model": "deepseek-chat",
                "input": "please read"
            })),
        )
        .await
        .expect("create first response");
        let first = response_json(first).await;
        let first_id = first["id"].as_str().expect("first id");
        assert_eq!(first["output"][0]["type"], "function_call");
        assert_eq!(first["output"][0]["call_id"], "call_1");
        assert_eq!(first["output"][0]["name"], "read_file");

        let second = create_response(
            State(state.clone()),
            axum::Json(json!({
                "model": "deepseek-chat",
                "previous_response_id": first_id,
                "input": [
                    {
                        "type": "function_call_output",
                        "call_id": "call_1",
                        "output": "file text"
                    }
                ]
            })),
        )
        .await
        .expect("create second response");
        let second = response_json(second).await;

        assert_eq!(second["output"][0]["content"][0]["text"], "tool accepted");

        let tool_call_count: Option<i64> = sqlx::query_scalar(
            "SELECT tool_call_count FROM request_logs WHERE model_requested = 'deepseek-chat' ORDER BY created_at ASC LIMIT 1",
        )
        .fetch_one(&state.db)
        .await
        .expect("load tool call count");

        assert_eq!(tool_call_count, Some(1));
    }

    #[tokio::test]
    async fn rejects_unknown_previous_response_id() {
        let upstream = spawn_chat_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
        };

        let error = create_response(
            State(state),
            axum::Json(json!({
                "model": "deepseek-chat",
                "previous_response_id": "resp_missing",
                "input": "second user"
            })),
        )
        .await
        .expect_err("unknown previous response id should fail");

        assert!(format!("{error:?}").contains("previous_response_id resp_missing 不存在"));
    }

    #[test]
    fn encodes_upstream_text_chunks_as_responses_sse_events() {
        let encoded = responses_sse_from_text_chunks(&["hel", "lo"]);

        assert!(encoded.contains("event: response.output_text.delta\ndata: hel\n\n"));
        assert!(encoded.contains("event: response.output_text.delta\ndata: lo\n\n"));
        assert!(encoded.contains("event: response.completed\ndata: {}\n\n"));
        assert!(encoded.ends_with("data: [DONE]\n\n"));
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
                            "content": "upstream hello"
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

    async fn spawn_streaming_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            use axum::response::IntoResponse;

            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["stream"], true);
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                 data: [DONE]\n\n",
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

    async fn spawn_delayed_streaming_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            use axum::response::IntoResponse;

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

    async fn spawn_native_responses_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "gpt-4.1");
            assert_eq!(payload["input"], "hello");
            Json(json!({
                "id": "resp_native",
                "object": "response",
                "model": "gpt-4.1",
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "native response"
                            }
                        ]
                    }
                ],
                "usage": {
                    "input_tokens": 3,
                    "output_tokens": 4
                }
            }))
        }

        let app = Router::new().route("/responses", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_ollama_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "llama3.2");
            assert_eq!(payload["stream"], false);
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");
            Json(json!({
                "model": "llama3.2",
                "message": {
                    "role": "assistant",
                    "content": "ollama hello"
                },
                "done": true,
                "prompt_eval_count": 3,
                "eval_count": 4
            }))
        }

        let app = Router::new().route("/chat", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_history_asserting_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            let messages = payload["messages"].as_array().expect("messages");
            let content = if messages.len() == 1 {
                assert_eq!(messages[0]["content"], "first user");
                "first assistant"
            } else {
                assert_eq!(messages.len(), 3);
                assert_eq!(messages[0]["role"], "user");
                assert_eq!(messages[0]["content"], "first user");
                assert_eq!(messages[1]["role"], "assistant");
                assert_eq!(messages[1]["content"], "first assistant");
                assert_eq!(messages[2]["role"], "user");
                assert_eq!(messages[2]["content"], "second user");
                "history accepted"
            };

            Json(json!({
                "id": "chatcmpl_history",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": content
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

    async fn spawn_tool_roundtrip_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            let messages = payload["messages"].as_array().expect("messages");
            if messages.len() == 1 {
                assert_eq!(messages[0]["role"], "user");
                return Json(json!({
                    "id": "chatcmpl_tool",
                    "model": payload["model"],
                    "choices": [
                        {
                            "message": {
                                "role": "assistant",
                                "content": null,
                                "reasoning_content": "Need to inspect the file before answering.",
                                "tool_calls": [
                                    {
                                        "id": "call_1",
                                        "type": "function",
                                        "function": {
                                            "name": "read_file",
                                            "arguments": "{\"path\":\"Cargo.toml\"}"
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }));
            }

            assert_eq!(messages.len(), 3);
            assert_eq!(messages[0]["role"], "user");
            assert_eq!(messages[0]["content"], "please read");
            assert_eq!(messages[1]["role"], "assistant");
            assert_eq!(
                messages[1]["reasoning_content"],
                "Need to inspect the file before answering."
            );
            assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
            assert_eq!(messages[2]["role"], "tool");
            assert_eq!(messages[2]["tool_call_id"], "call_1");
            assert_eq!(messages[2]["content"], "file text");

            Json(json!({
                "id": "chatcmpl_tool_done",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "tool accepted"
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

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read body");
        serde_json::from_slice(&body).expect("json body")
    }

    async fn spawn_failing_chat_upstream() -> String {
        async fn handler() -> (axum::http::StatusCode, Json<serde_json::Value>) {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "message": "upstream failed" } })),
            )
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
