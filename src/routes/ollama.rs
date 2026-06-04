use axum::{extract::State, routing::post, Json, Router};
use serde_json::Value;

use crate::{
    db,
    error::AppError,
    stats::{insert_request_log, RequestLogInsert},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/chat", post(create_ollama_chat))
}

async fn create_ollama_chat(
    State(state): State<AppState>,
    Json(mut payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
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
            is_streaming,
            input_tokens: response.get("prompt_eval_count").and_then(Value::as_i64),
            output_tokens: response.get("eval_count").and_then(Value::as_i64),
            reasoning_tokens: None,
            latency_ms: started_at.elapsed().as_millis() as i64,
            upstream_latency_ms: Some(upstream_latency_ms),
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
        },
    )
    .await?;

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use axum::{middleware, routing::post, Json, Router};
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
            db,
            client: reqwest::Client::new(),
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
}
