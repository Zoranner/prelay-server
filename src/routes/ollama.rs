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
    db,
    error::AppError,
    routes::stream_stats::record_first_chunk,
    stats::{insert_request_log, RequestLogInsert},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/chat", post(create_ollama_chat))
}

async fn create_ollama_chat(
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
    let resolved = db::get_provider_by_model(&state.db, &model, "ollama_native")
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("Ollama 模型 {model} 未配置")))?;
    let provider = resolved.provider;
    if provider.provider_type != "ollama_native" {
        return Err(AppError::BadRequest(format!("Ollama 模型 {model} 未配置")));
    }
    let model_upstream = resolved.model_upstream;
    let upstream_url = format!("{}/chat", provider.base_url.trim_end_matches('/'));
    payload["model"] = Value::String(model_upstream.clone());
    payload["stream"] = Value::Bool(is_streaming);

    let upstream_started_at = std::time::Instant::now();
    let upstream_response = state
        .client
        .post(upstream_url)
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
                protocol_in: "ollama_native".to_string(),
                protocol_out: "ollama_native".to_string(),
                protocol_upstream: "ollama_native".to_string(),
                provider_id: provider.id,
                provider_name: provider.name,
                model_requested: model.clone(),
                model_upstream,
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
            protocol_in: "ollama_native".to_string(),
            protocol_out: "ollama_native".to_string(),
            protocol_upstream: "ollama_native".to_string(),
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
            upstream_request_id: None,
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
        return Ok(([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response());
    }

    let response = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    insert_request_log(
        &state.db,
        RequestLogInsert {
            protocol_in: "ollama_native".to_string(),
            protocol_out: "ollama_native".to_string(),
            protocol_upstream: "ollama_native".to_string(),
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
            input_tokens: response.get("prompt_eval_count").and_then(Value::as_i64),
            output_tokens: response.get("eval_count").and_then(Value::as_i64),
            reasoning_tokens: None,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
            metadata_json: None,
        },
    )
    .await?;

    Ok(Json(response).into_response())
}

#[cfg(test)]
mod tests {
    use axum::{body::Bytes, middleware, routing::post, Json, Router};
    use futures::StreamExt;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::net::TcpListener;

    use crate::{db, AppState};

    #[tokio::test]
    async fn rejects_unauthenticated_ollama_chat_request() {
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
            .post(format!("http://{addr}/api/chat"))
            .json(&json!({
                "model": "llama3.2",
                "stream": false,
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
    async fn forwards_ollama_chat_request_to_native_upstream() {
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
        let app = Router::new().merge(super::router().with_state(state));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/chat"))
            .json(&json!({
                "model": "llama3.2",
                "stream": false,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = response.json().await.expect("parse response json");
        assert_eq!(body["model"], "llama3.2");
        assert_eq!(body["message"]["role"], "assistant");
        assert_eq!(body["message"]["content"], "ollama hello");

        server.abort();
    }

    #[tokio::test]
    async fn records_successful_ollama_chat_request_log() {
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
        let app = Router::new().merge(super::router().with_state(state.clone()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("read test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/chat"))
            .json(&json!({
                "model": "llama3.2",
                "stream": false,
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
    async fn streams_ollama_native_chat_without_waiting_for_upstream_done() {
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
            .post(format!("http://{addr}/api/chat"))
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
        assert!(first_chunk.contains("\"content\":\"hel\""));
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

            let stream = futures::stream::unfold(0, |state| async move {
                match state {
                    0 => Some((
                        Ok::<_, std::io::Error>(Bytes::from_static(
                            b"{\"model\":\"llama3.2\",\"message\":{\"role\":\"assistant\",\"content\":\"hel\"},\"done\":false}\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, std::io::Error>(Bytes::from_static(
                                b"{\"model\":\"llama3.2\",\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":true}\n",
                            )),
                            2,
                        ))
                    }
                    _ => None,
                }
            });

            axum::response::Response::builder()
                .header("content-type", "application/x-ndjson")
                .body(axum::body::Body::from_stream(stream))
                .expect("build streaming response")
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
