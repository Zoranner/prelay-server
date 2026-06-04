use axum::{extract::State, routing::post, Json, Router};
use serde_json::Value;

use crate::{
    bridge::{
        anthropic_decode::decode_anthropic_request, anthropic_encode::encode_anthropic_response,
    },
    db,
    error::AppError,
    providers::chat_completions::{decode_chat_response, encode_chat_request},
    stats::{insert_request_log, RequestLogInsert},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/messages", post(create_message))
}

async fn create_message(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let started_at = std::time::Instant::now();
    let request = decode_anthropic_request(payload)?;
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let provider = db::list_configs(&state.db)
        .await?
        .into_iter()
        .find(|provider| provider.name == request.model)
        .ok_or_else(|| AppError::BadRequest(format!("模型 {} 未配置", request.model)))?;
    let upstream_url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );
    let upstream_response = state
        .client
        .post(upstream_url)
        .bearer_auth(&provider.api_key)
        .json(&encode_chat_request(&request))
        .send()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;

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
                is_streaming,
                input_tokens: None,
                output_tokens: None,
                latency_ms: started_at.elapsed().as_millis() as i64,
            },
        )
        .await?;
        return Err(AppError::BadRequest(format!("上游请求失败: {}", status)));
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let response = decode_chat_response(upstream_json)?;
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
            is_streaming,
            input_tokens: response.usage.as_ref().and_then(|usage| usage.input_tokens),
            output_tokens: response
                .usage
                .as_ref()
                .and_then(|usage| usage.output_tokens),
            latency_ms: started_at.elapsed().as_millis() as i64,
        },
    )
    .await?;

    Ok(Json(encode_anthropic_response(response)))
}

#[cfg(test)]
mod tests {
    use axum::{routing::post, Json, Router};
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::net::TcpListener;

    use crate::{db, AppState};

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
}
