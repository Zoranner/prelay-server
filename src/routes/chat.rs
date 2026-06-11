use axum::{
    body::Body,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use futures::TryStreamExt;
use serde_json::Value;

use crate::{
    bridge::{compatibility::first_rejection, stream::ollama_chat_ndjson_response_to_chat_sse},
    db,
    error::AppError,
    providers::{
        chat_completions::{decode_chat_request, encode_chat_response},
        ollama::{decode_ollama_chat_response, encode_ollama_chat_request},
        spec::{ProviderSpec, UpstreamProtocol},
    },
    routes::{stream_stats::record_first_chunk, upstream_observability::upstream_observability},
    stats::{insert_request_log, RequestLogInsert},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/chat/completions", post(create_chat_completion))
}

async fn create_chat_completion(
    State(state): State<AppState>,
    Json(mut payload): Json<Value>,
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
    let resolved = db::get_provider_by_model(&state.db, &model, "chat_completions")
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("模型 {model} 未配置")))?;
    let provider = resolved.provider;
    let model_upstream = resolved.model_upstream;
    let provider_spec = ProviderSpec::from_provider_config(&provider);
    if provider_spec.protocol == UpstreamProtocol::OllamaNative {
        payload["model"] = Value::String(model_upstream);
        return create_ollama_chat_completion(&state, payload, provider, model, started_at).await;
    }

    payload["model"] = Value::String(model_upstream.clone());
    let upstream_url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );
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
        let observability_headers = upstream_response.headers().clone();
        let error_body = upstream_response.text().await.ok();
        let observability = upstream_observability(&observability_headers, error_body.as_deref());
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "chat_completions".to_string(),
                protocol_out: "chat_completions".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
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
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: observability.request_id,
                metadata_json: None,
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
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
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id,
            metadata_json: None,
        };
        let body = Body::from_stream(record_first_chunk(
            state.db.clone(),
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
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "chat_completions".to_string(),
            protocol_out: "chat_completions".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
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
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id,
            metadata_json: None,
        },
    )
    .await?;

    Ok(Json(response).into_response())
}

async fn create_ollama_chat_completion(
    state: &AppState,
    payload: Value,
    provider: crate::models::ProviderConfig,
    model_requested: String,
    started_at: std::time::Instant,
) -> Result<Response, AppError> {
    let request = decode_chat_request(payload)?;
    let is_streaming = request.stream;
    if let Some(rejection) = first_rejection(&provider, &request) {
        let error_message = rejection.reason.to_string();
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "chat_completions".to_string(),
                protocol_out: "chat_completions".to_string(),
                protocol_upstream: "ollama_native".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested,
                model_upstream: request.model,
                status: "failed".to_string(),
                http_status: 400,
                error_code: Some(rejection.error_code().to_string()),
                error_message: Some(error_message.clone()),
                is_streaming,
                input_tokens: None,
                output_tokens: None,
                reasoning_tokens: None,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
                metadata_json: Some(rejection.metadata_json()),
            },
        )
        .await?;
        return Err(AppError::BadRequest(error_message));
    }

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
                protocol_in: "chat_completions".to_string(),
                protocol_out: "chat_completions".to_string(),
                protocol_upstream: "ollama_native".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested,
                model_upstream: request.model,
                status: "failed".to_string(),
                http_status: status.as_u16() as i64,
                error_code: None,
                error_message: None,
                is_streaming,
                input_tokens: None,
                output_tokens: None,
                reasoning_tokens: None,
                latency_ms: started_at.elapsed().as_millis() as i64,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
                metadata_json: None,
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
    }

    if is_streaming {
        let log = RequestLogInsert {
            protocol_in: "chat_completions".to_string(),
            protocol_out: "chat_completions".to_string(),
            protocol_upstream: "ollama_native".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            model_requested,
            model_upstream: request.model,
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming,
            input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
            metadata_json: None,
        };
        let body = Body::from_stream(record_first_chunk(
            state.db.clone(),
            ollama_chat_ndjson_response_to_chat_sse(upstream_response),
            log,
            started_at,
        ));
        return Ok(([(header::CONTENT_TYPE, "text/event-stream")], body).into_response());
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let response = decode_ollama_chat_response(upstream_json)?;
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "chat_completions".to_string(),
            protocol_out: "chat_completions".to_string(),
            protocol_upstream: "ollama_native".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            model_requested,
            model_upstream: response.model.clone(),
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
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
            tool_call_count: Some(0),
            upstream_request_id: None,
            metadata_json: None,
        },
    )
    .await?;

    Ok(Json(encode_chat_response(response)).into_response())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body, extract::State, middleware, response::IntoResponse, routing::post, Json, Router,
    };
    use bytes::Bytes;
    use futures::StreamExt;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::{convert::Infallible, time::Duration};
    use tokio::net::TcpListener;

    use super::create_chat_completion;
    use crate::{db, AppState};

    #[tokio::test]
    async fn rejects_unauthenticated_chat_completion_request() {
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
            .post(format!("http://{addr}/v1/chat/completions"))
            .json(&json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");

        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        server.abort();
    }

    #[tokio::test]
    async fn forwards_chat_completion_request_to_configured_upstream() {
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
            admin_token: None,
        };

        let response = create_chat_completion(
            State(state),
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
    async fn resolves_chat_completion_model_alias_to_upstream_model() {
        let upstream = spawn_alias_chat_upstream().await;
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
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        db::create_model_alias(
            &db,
            "coder",
            &provider.id,
            "deepseek-chat",
            &["chat_completions"],
        )
        .await
        .expect("create alias");
        let state = AppState {
            db,
            client: reqwest::Client::new(),
            admin_token: None,
        };

        let response = create_chat_completion(
            State(state),
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
    async fn forwards_chat_completion_request_to_ollama_native_upstream() {
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
            db: db.clone(),
            client: reqwest::Client::new(),
            admin_token: None,
        };

        let response = create_chat_completion(
            State(state),
            axum::Json(json!({
                "model": "llama3.2",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");

        let response = response_json(response).await;

        assert_eq!(response["object"], "chat.completion");
        assert_eq!(response["model"], "llama3.2");
        assert_eq!(response["choices"][0]["message"]["content"], "ollama hello");
        assert_eq!(response["usage"]["prompt_tokens"], 3);
        assert_eq!(response["usage"]["completion_tokens"], 4);
        let protocol_upstream: String =
            sqlx::query_scalar("SELECT protocol_upstream FROM request_logs LIMIT 1")
                .fetch_one(&db)
                .await
                .expect("load request log");
        assert_eq!(protocol_upstream, "ollama_native");
    }

    #[tokio::test]
    async fn streams_ollama_native_chunks_as_chat_completion_sse() {
        let upstream = spawn_streaming_ollama_upstream().await;
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
            db: db.clone(),
            client: reqwest::Client::new(),
            admin_token: None,
        };
        let app = Router::new().merge(super::router().with_state(state));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/chat/completions"))
            .json(&json!({
                "model": "llama3.2",
                "stream": true,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let mut stream = response.bytes_stream();
        let first = stream
            .next()
            .await
            .expect("first response chunk")
            .expect("first response chunk ok");
        let first = String::from_utf8(first.to_vec()).expect("first chunk utf8");

        assert!(first.contains("\"content\":\"hel\""));
        let first_token_ms: Option<i64> =
            sqlx::query_scalar("SELECT first_token_ms FROM request_logs LIMIT 1")
                .fetch_one(&db)
                .await
                .expect("load first token metric");
        assert!(first_token_ms.is_some());

        server.abort();
    }

    #[tokio::test]
    async fn rejects_model_alias_when_chat_protocol_is_not_allowed() {
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
            "http://127.0.0.1:1",
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

        let error = create_chat_completion(
            State(state),
            axum::Json(json!({
                "model": "coder",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect_err("blocked alias should fail before upstream");

        assert!(format!("{error:?}").contains("模型 coder 未配置"));
    }

    #[tokio::test]
    async fn records_successful_chat_completion_request_log() {
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
            admin_token: None,
        };

        let _response = create_chat_completion(
            State(state.clone()),
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");
        let overview = crate::stats::overview(&state.db)
            .await
            .expect("stats overview");

        assert_eq!(overview.total_requests, 1);
        assert_eq!(overview.successful_requests, 1);
        assert_eq!(overview.input_tokens, 3);
        assert_eq!(overview.output_tokens, 4);
    }

    #[tokio::test]
    async fn records_successful_chat_completion_upstream_request_id() {
        let upstream = spawn_request_id_chat_upstream().await;
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
            db: db.clone(),
            client: reqwest::Client::new(),
            admin_token: None,
        };

        let _response = create_chat_completion(
            State(state),
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");

        let upstream_request_id: Option<String> =
            sqlx::query_scalar("SELECT upstream_request_id FROM request_logs LIMIT 1")
                .fetch_one(&db)
                .await
                .expect("load upstream request id");

        assert_eq!(upstream_request_id.as_deref(), Some("req_chat_123"));
    }

    #[tokio::test]
    async fn records_failed_chat_completion_upstream_request_id_and_error_message() {
        let upstream = spawn_error_chat_upstream().await;
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
            db: db.clone(),
            client: reqwest::Client::new(),
            admin_token: None,
        };

        let error = create_chat_completion(
            State(state),
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
        let row: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT upstream_request_id, error_message FROM request_logs LIMIT 1")
                .fetch_one(&db)
                .await
                .expect("load failed request log");

        assert_eq!(row.0.as_deref(), Some("cf-ray-123"));
        assert_eq!(row.1.as_deref(), Some("provider overloaded"));
    }

    #[tokio::test]
    async fn streams_chat_completion_without_waiting_for_upstream_done() {
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
            db: db.clone(),
            client: reqwest::Client::new(),
            admin_token: None,
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
            .post(format!("http://{addr}/v1/chat/completions"))
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

        let overview = crate::stats::overview(&db).await.expect("stats overview");
        assert_eq!(overview.total_requests, 1);
        assert_eq!(overview.successful_requests, 1);
        let first_token_ms: Option<i64> =
            sqlx::query_scalar("SELECT first_token_ms FROM request_logs LIMIT 1")
                .fetch_one(&db)
                .await
                .expect("load first token metric");
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

    async fn spawn_streaming_ollama_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            assert_eq!(payload["model"], "llama3.2");
            assert_eq!(payload["stream"], true);
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");

            let stream = futures::stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"{\"message\":{\"role\":\"assistant\",\"content\":\"hel\"},\"done\":false}\n",
                        )),
                        1,
                    )),
                    1 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"{\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":true}\n",
                        )),
                        2,
                    )),
                    _ => None,
                }
            });

            (
                [(axum::http::header::CONTENT_TYPE, "application/x-ndjson")],
                Body::from_stream(stream),
            )
                .into_response()
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
}
