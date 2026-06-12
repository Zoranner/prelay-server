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
    bridge::{
        anthropic_decode::decode_anthropic_request_with_diagnostics,
        anthropic_encode::encode_anthropic_response,
        stream::{
            chat_sse_response_to_anthropic_messages_sse,
            responses_sse_response_to_anthropic_messages_sse,
        },
    },
    db,
    error::AppError,
    providers::chat_completions::{decode_chat_response, encode_chat_request},
    providers::responses::{decode_responses_response, encode_responses_request},
    providers::spec::{ProviderSpec, UpstreamProtocol},
    routes::{request_metadata::build_request_metadata, stream_stats::record_first_chunk},
    stats::{insert_request_log, RequestLogInsert},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/messages", post(create_message))
}

async fn create_message(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let started_at = std::time::Instant::now();
    let original_payload = payload.clone();
    let decoded_request = decode_anthropic_request_with_diagnostics(payload)?;
    let request = decoded_request.request;
    let diagnostics = decoded_request.diagnostics;
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let resolved = db::get_provider_by_model(&state.db, &request.model, "anthropic_messages")
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("模型 {} 未配置", request.model)))?;
    let provider = resolved.provider;
    let provider_spec = ProviderSpec::from_provider_config(&provider);
    let model_upstream = resolved.model_upstream;
    let metadata_json = build_request_metadata(
        "anthropic_messages",
        "anthropic_messages",
        provider_spec.protocol,
        &model_requested,
        &model_upstream,
        diagnostics,
    )?;
    let mut upstream_payload = original_payload.clone();
    upstream_payload["model"] = Value::String(model_upstream.clone());
    let mut request = request;
    request.model = model_upstream.clone();
    if provider_spec.protocol == UpstreamProtocol::AnthropicMessages {
        return create_native_anthropic_message(
            &state,
            upstream_payload,
            provider,
            model_requested,
            is_streaming,
            metadata_json,
            started_at,
        )
        .await
        .map(IntoResponse::into_response);
    }
    if provider_spec.protocol == UpstreamProtocol::Responses {
        return create_responses_anthropic_message(
            &state,
            request,
            provider,
            model_requested,
            is_streaming,
            metadata_json,
            started_at,
        )
        .await
        .map(IntoResponse::into_response);
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
        .json(&encode_chat_request(&request))
        .send()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "anthropic_messages".to_string(),
                protocol_out: "anthropic_messages".to_string(),
                protocol_upstream: "chat_completions".to_string(),
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
                metadata_json: Some(metadata_json.clone()),
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
    }

    if is_streaming {
        let log = RequestLogInsert {
            protocol_in: "anthropic_messages".to_string(),
            protocol_out: "anthropic_messages".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            model_requested,
            model_upstream: request.model.clone(),
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
            metadata_json: Some(metadata_json.clone()),
        };
        let stream =
            chat_sse_response_to_anthropic_messages_sse(upstream_response, request.model.clone());
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(record_first_chunk(
                state.db.clone(),
                stream,
                log,
                started_at,
            )))
            .map_err(|error| AppError::Internal(error.into()));
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let response = decode_chat_response(upstream_json)?;
    let reasoning_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| usage.reasoning_tokens);
    let tool_call_count = count_tool_calls(&response);
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "anthropic_messages".to_string(),
            protocol_out: "anthropic_messages".to_string(),
            protocol_upstream: "chat_completions".to_string(),
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
            reasoning_tokens,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: Some(tool_call_count),
            upstream_request_id: None,
            metadata_json: Some(metadata_json),
        },
    )
    .await?;

    Ok(Json(encode_anthropic_response(response)).into_response())
}

async fn create_responses_anthropic_message(
    state: &AppState,
    request: crate::bridge::internal::InternalRequest,
    provider: crate::models::ProviderConfig,
    model_requested: String,
    is_streaming: bool,
    metadata_json: String,
    started_at: std::time::Instant,
) -> Result<Response, AppError> {
    let upstream_url = format!("{}/responses", provider.base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&encode_responses_request(&request))
        .send()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let upstream_latency_ms = upstream_started_at.elapsed().as_millis() as i64;

    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        insert_request_log(
            &state.db,
            RequestLogInsert {
                protocol_in: "anthropic_messages".to_string(),
                protocol_out: "anthropic_messages".to_string(),
                protocol_upstream: "responses".to_string(),
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
                metadata_json: Some(metadata_json.clone()),
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
    }

    if is_streaming {
        let log = RequestLogInsert {
            protocol_in: "anthropic_messages".to_string(),
            protocol_out: "anthropic_messages".to_string(),
            protocol_upstream: "responses".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            model_requested,
            model_upstream: request.model.clone(),
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
            metadata_json: Some(metadata_json.clone()),
        };
        let stream =
            responses_sse_response_to_anthropic_messages_sse(upstream_response, request.model);
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(record_first_chunk(
                state.db.clone(),
                stream,
                log,
                started_at,
            )))
            .map_err(|error| AppError::Internal(error.into()));
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let response = decode_responses_response(upstream_json)?;
    let reasoning_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| usage.reasoning_tokens);
    let tool_call_count = count_tool_calls(&response);
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "anthropic_messages".to_string(),
            protocol_out: "anthropic_messages".to_string(),
            protocol_upstream: "responses".to_string(),
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
            reasoning_tokens,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: Some(tool_call_count),
            upstream_request_id: None,
            metadata_json: Some(metadata_json),
        },
    )
    .await?;

    Ok(Json(encode_anthropic_response(response)).into_response())
}

async fn create_native_anthropic_message(
    state: &AppState,
    payload: Value,
    provider: crate::models::ProviderConfig,
    model_requested: String,
    is_streaming: bool,
    metadata_json: String,
    started_at: std::time::Instant,
) -> Result<Response, AppError> {
    let upstream_url = format!("{}/messages", provider.base_url.trim_end_matches('/'));
    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
        .header("x-api-key", &provider.api_key)
        .header("anthropic-version", "2023-06-01")
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
                protocol_in: "anthropic_messages".to_string(),
                protocol_out: "anthropic_messages".to_string(),
                protocol_upstream: "anthropic_messages".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested,
                model_upstream: "unknown".to_string(),
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
                metadata_json: Some(metadata_json.clone()),
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
    }

    if is_streaming {
        let model_upstream = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let log = RequestLogInsert {
            protocol_in: "anthropic_messages".to_string(),
            protocol_out: "anthropic_messages".to_string(),
            protocol_upstream: "anthropic_messages".to_string(),
            provider_id: provider.id,
            provider_name: provider.name,
            model_requested,
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
            upstream_request_id: None,
            metadata_json: Some(metadata_json.clone()),
        };

        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(record_first_chunk(
                state.db.clone(),
                upstream_response
                    .bytes_stream()
                    .map_err(std::io::Error::other),
                log,
                started_at,
            )))
            .map_err(|error| AppError::Internal(error.into()));
    }

    let response = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "anthropic_messages".to_string(),
            protocol_out: "anthropic_messages".to_string(),
            protocol_upstream: "anthropic_messages".to_string(),
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
            error_code: None,
            error_message: None,
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
            metadata_json: Some(metadata_json),
        },
    )
    .await?;

    Ok(Json(response).into_response())
}

fn count_tool_calls(response: &crate::bridge::internal::InternalResponse) -> i64 {
    response
        .output
        .iter()
        .filter(|item| item.is_tool_call())
        .count() as i64
}

#[cfg(test)]
mod tests {
    use axum::{middleware, routing::post, Json, Router};
    use futures::{StreamExt, TryStreamExt};
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::net::TcpListener;

    use crate::{db, AppState};

    #[tokio::test]
    async fn rejects_unauthenticated_anthropic_messages_request() {
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
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "deepseek-chat",
                "max_tokens": 1024,
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
    async fn forwards_anthropic_messages_request_to_chat_completions_upstream() {
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
        let app = Router::new().merge(super::router().with_state(state));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "deepseek-chat",
                "max_tokens": 1024,
                "system": "Be concise.",
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": "hello" }
                        ]
                    }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        assert_eq!(body["type"], "message");
        assert_eq!(body["role"], "assistant");
        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["content"][0]["type"], "text");
        assert_eq!(body["content"][0]["text"], "anthropic hello");
        assert_eq!(body["usage"]["input_tokens"], 3);
        assert_eq!(body["usage"]["output_tokens"], 4);

        server.abort();
    }

    #[tokio::test]
    async fn forwards_anthropic_messages_request_to_responses_upstream_for_openai_provider() {
        let upstream = spawn_responses_upstream().await;
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
            admin_token: None,
        };
        let app = Router::new().merge(super::router().with_state(state.clone()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "gpt-4.1",
                "max_tokens": 1024,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        assert_eq!(body["type"], "message");
        assert_eq!(body["role"], "assistant");
        assert_eq!(body["model"], "gpt-4.1");
        assert_eq!(body["content"][0]["type"], "text");
        assert_eq!(body["content"][0]["text"], "responses hello");
        assert_eq!(body["usage"]["input_tokens"], 3);
        assert_eq!(body["usage"]["output_tokens"], 4);

        let row: (String, Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT protocol_upstream, input_tokens, output_tokens FROM request_logs LIMIT 1",
        )
        .fetch_one(&state.db)
        .await
        .expect("load request log");
        assert_eq!(row.0, "responses");
        assert_eq!(row.1, Some(3));
        assert_eq!(row.2, Some(4));

        server.abort();
    }

    #[tokio::test]
    async fn streams_responses_sse_as_anthropic_messages_sse() {
        let upstream = spawn_streaming_responses_upstream().await;
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
            admin_token: None,
        };
        let app = Router::new().merge(super::router().with_state(state.clone()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "gpt-4.1",
                "max_tokens": 1024,
                "stream": true,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let started_at = std::time::Instant::now();
        let mut stream = response.bytes_stream();
        let first_chunk = stream
            .next()
            .await
            .expect("receive first stream chunk")
            .expect("first stream chunk ok");
        let first_chunk_elapsed = started_at.elapsed();
        let first_chunk = String::from_utf8(first_chunk.to_vec()).expect("utf8 stream chunk");

        assert!(
            first_chunk_elapsed < std::time::Duration::from_millis(200),
            "first chunk took {first_chunk_elapsed:?}"
        );
        assert!(first_chunk.contains("event: content_block_delta"));
        assert!(first_chunk.contains("hel"));

        let body = stream
            .map(|chunk| {
                chunk.map(|chunk| String::from_utf8(chunk.to_vec()).expect("utf8 stream chunk"))
            })
            .try_collect::<String>()
            .await
            .expect("read remaining stream");
        let body = format!("{first_chunk}{body}");
        assert!(body.contains("event: content_block_delta"));
        assert!(body.contains("lo"));
        assert!(body.contains("event: message_stop"));

        let row: (String, String, i64, Option<i64>) = sqlx::query_as(
            "SELECT protocol_upstream, status, http_status, first_token_ms FROM request_logs LIMIT 1",
        )
        .fetch_one(&state.db)
        .await
        .expect("load request log");

        assert_eq!(row.0, "responses");
        assert_eq!(row.1, "success");
        assert_eq!(row.2, 200);
        assert!(row.3.is_some());

        server.abort();
    }

    #[tokio::test]
    async fn forwards_anthropic_messages_request_to_native_upstream() {
        let upstream = spawn_native_anthropic_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "claude-sonnet",
            "anthropic_compatible",
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
        let app = Router::new().merge(super::router().with_state(state));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "claude-sonnet",
                "max_tokens": 1024,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        assert_eq!(body["id"], "msg_native");
        assert_eq!(body["model"], "claude-sonnet");
        assert_eq!(body["content"][0]["text"], "native hello");

        server.abort();
    }

    #[tokio::test]
    async fn records_successful_anthropic_messages_request_log() {
        let upstream = spawn_user_only_chat_upstream().await;
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
        let app = Router::new().merge(super::router().with_state(state.clone()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "deepseek-chat",
                "max_tokens": 1024,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let overview = crate::stats::overview(&state.db)
            .await
            .expect("stats overview");

        assert_eq!(overview.total_requests, 1);
        assert_eq!(overview.successful_requests, 1);
        assert_eq!(overview.input_tokens, 3);
        assert_eq!(overview.output_tokens, 4);

        server.abort();
    }

    #[tokio::test]
    async fn records_anthropic_decode_diagnostics_in_request_metadata() {
        let upstream = spawn_user_only_chat_upstream().await;
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
        let app = Router::new().merge(super::router().with_state(state.clone()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "deepseek-chat",
                "max_tokens": 1024,
                "messages": [
                    { "role": "planner", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let metadata_json: Option<String> =
            sqlx::query_scalar("SELECT metadata_json FROM request_logs LIMIT 1")
                .fetch_one(&state.db)
                .await
                .expect("load metadata");
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata_json.expect("metadata json")).expect("parse metadata");

        assert_eq!(metadata["bridge"]["protocol_in"], "anthropic_messages");
        assert_eq!(metadata["bridge"]["protocol_upstream"], "chat_completions");
        assert_eq!(metadata["bridge"]["model_requested"], "deepseek-chat");
        assert_eq!(metadata["bridge"]["model_upstream"], "deepseek-chat");
        assert_eq!(metadata["diagnostics"][0]["code"], "anthropic.role.unknown");
        assert_eq!(metadata["diagnostics"][0]["severity"], "warning");

        server.abort();
    }

    #[tokio::test]
    async fn bridges_chat_tool_call_to_anthropic_tool_use() {
        let upstream = spawn_tool_call_chat_upstream().await;
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
        let app = Router::new().merge(super::router().with_state(state));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "deepseek-chat",
                "max_tokens": 1024,
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Read a file",
                        "input_schema": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            }
                        }
                    }
                ],
                "messages": [
                    { "role": "user", "content": "read Cargo.toml" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        assert_eq!(body["stop_reason"], "tool_use");
        assert_eq!(body["content"][0]["type"], "tool_use");
        assert_eq!(body["content"][0]["id"], "call_1");
        assert_eq!(body["content"][0]["name"], "read_file");
        assert_eq!(body["content"][0]["input"]["path"], "Cargo.toml");

        server.abort();
    }

    #[tokio::test]
    async fn bridges_anthropic_tool_result_to_chat_tool_message() {
        let upstream = spawn_tool_result_chat_upstream().await;
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
        let app = Router::new().merge(super::router().with_state(state));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "deepseek-chat",
                "max_tokens": 1024,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": "call_1",
                                "content": "file text"
                            }
                        ]
                    }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        assert_eq!(body["content"][0]["type"], "text");
        assert_eq!(body["content"][0]["text"], "tool accepted");

        server.abort();
    }

    #[tokio::test]
    async fn streams_chat_completions_text_delta_without_waiting_for_upstream_done() {
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
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "deepseek-chat",
                "max_tokens": 1024,
                "stream": true,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let started_at = std::time::Instant::now();
        let mut stream = response.bytes_stream();
        let first_chunk = stream
            .next()
            .await
            .expect("receive first stream chunk")
            .expect("first stream chunk ok");
        let first_chunk_elapsed = started_at.elapsed();
        let first_chunk = String::from_utf8(first_chunk.to_vec()).expect("utf8 stream chunk");

        assert!(
            first_chunk_elapsed < std::time::Duration::from_millis(200),
            "first chunk took {first_chunk_elapsed:?}"
        );
        assert!(first_chunk.contains("event: content_block_delta"));
        assert!(first_chunk.contains("hel"));

        server.abort();
    }

    #[tokio::test]
    async fn streams_native_anthropic_messages_sse_without_waiting_for_upstream_done() {
        let upstream = spawn_streaming_native_anthropic_upstream().await;
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        db::create_config(
            &db,
            "claude-sonnet",
            "anthropic_compatible",
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
        let app = Router::new().merge(super::router().with_state(state));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/v1/messages"))
            .json(&json!({
                "model": "claude-sonnet",
                "max_tokens": 1024,
                "stream": true,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let started_at = std::time::Instant::now();
        let mut stream = response.bytes_stream();
        let first_chunk = stream
            .next()
            .await
            .expect("receive first stream chunk")
            .expect("first stream chunk ok");
        let first_chunk_elapsed = started_at.elapsed();
        let first_chunk = String::from_utf8(first_chunk.to_vec()).expect("utf8 stream chunk");

        assert!(
            first_chunk_elapsed < std::time::Duration::from_millis(200),
            "first chunk took {first_chunk_elapsed:?}"
        );
        assert!(first_chunk.contains("event: content_block_delta"));
        assert!(first_chunk.contains("hel"));

        server.abort();
    }

    async fn spawn_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["stream"], false);
            assert_eq!(payload["max_tokens"], 1024);
            assert_eq!(payload["messages"][0]["role"], "system");
            assert_eq!(payload["messages"][0]["content"], "Be concise.");
            assert_eq!(payload["messages"][1]["role"], "user");
            assert_eq!(payload["messages"][1]["content"], "hello");
            Json(json!({
                "id": "chatcmpl_anthropic",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "anthropic hello"
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

    async fn spawn_user_only_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["stream"], false);
            assert_eq!(payload["max_tokens"], 1024);
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");
            Json(json!({
                "id": "chatcmpl_anthropic",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "anthropic hello"
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

    async fn spawn_responses_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "gpt-4.1");
            assert_eq!(payload["stream"], false);
            assert_eq!(payload["max_output_tokens"], 1024);
            assert_eq!(payload["input"][0]["role"], "user");
            assert_eq!(payload["input"][0]["content"], "hello");
            Json(json!({
                "id": "resp_anthropic",
                "model": payload["model"],
                "output": [
                    {
                        "type": "message",
                        "id": "msg_1",
                        "role": "assistant",
                        "content": [
                            { "type": "output_text", "text": "responses hello" }
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

    async fn spawn_streaming_responses_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            assert_eq!(payload["model"], "gpt-4.1");
            assert_eq!(payload["stream"], true);
            assert_eq!(payload["max_output_tokens"], 1024);
            assert_eq!(payload["input"][0]["role"], "user");
            assert_eq!(payload["input"][0]["content"], "hello");

            let stream = futures::stream::unfold(0, |state| async move {
                match state {
                    0 => Some((
                        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                            b"event: response.output_text.delta\ndata: {\"delta\":\"hel\"}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                b"event: response.output_text.delta\ndata: {\"delta\":\"lo\"}\n\nevent: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });

            axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from_stream(stream))
                .expect("build streaming response")
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

    async fn spawn_native_anthropic_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "claude-sonnet");
            assert_eq!(payload["max_tokens"], 1024);
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");
            Json(json!({
                "id": "msg_native",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet",
                "content": [
                    { "type": "text", "text": "native hello" }
                ],
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 3,
                    "output_tokens": 4
                }
            }))
        }

        let app = Router::new().route("/messages", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream");
        let addr = listener.local_addr().expect("upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve upstream");
        });
        format!("http://{addr}")
    }

    async fn spawn_tool_call_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["tools"][0]["type"], "function");
            assert_eq!(payload["tools"][0]["function"]["name"], "read_file");
            assert_eq!(
                payload["tools"][0]["function"]["parameters"]["properties"]["path"]["type"],
                "string"
            );
            Json(json!({
                "id": "chatcmpl_tool",
                "model": payload["model"],
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": null,
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

    async fn spawn_tool_result_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["messages"][0]["role"], "tool");
            assert_eq!(payload["messages"][0]["tool_call_id"], "call_1");
            assert_eq!(payload["messages"][0]["content"], "file text");
            Json(json!({
                "id": "chatcmpl_tool_result",
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

    async fn spawn_streaming_chat_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            assert_eq!(payload["model"], "deepseek-chat");
            assert_eq!(payload["stream"], true);
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");

            let stream = futures::stream::unfold(0, |state| async move {
                match state {
                    0 => Some((
                        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                            b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\ndata: [DONE]\n\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });

            axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from_stream(stream))
                .expect("build streaming response")
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

    async fn spawn_streaming_native_anthropic_upstream() -> String {
        async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
            assert_eq!(payload["model"], "claude-sonnet");
            assert_eq!(payload["stream"], true);
            assert_eq!(payload["max_tokens"], 1024);
            assert_eq!(payload["messages"][0]["role"], "user");
            assert_eq!(payload["messages"][0]["content"], "hello");

            let stream = futures::stream::unfold(0, |state| async move {
                match state {
                    0 => Some((
                        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_native_stream\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                                b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });

            axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from_stream(stream))
                .expect("build streaming response")
        }

        let app = Router::new().route("/messages", post(handler));
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
