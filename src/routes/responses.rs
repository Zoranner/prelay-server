use axum::{
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
    },
    db,
    error::AppError,
    providers::chat_completions::{decode_chat_response, encode_chat_request},
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
    let request = decode_responses_request(payload)?;
    let model_requested = request.model.clone();
    let is_streaming = request.stream;
    let previous_response_id = request.previous_response_id.clone();
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
        .json(&encode_chat_request(
            &request_with_session_history(&state.db, request.clone()).await?,
        ))
        .send()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;

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
    let mut response = decode_chat_response(upstream_json)?;
    response.id = format!("resp_{}", Uuid::new_v4().simple());
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
            latency_ms: started_at.elapsed().as_millis() as i64,
        },
    )
    .await?;

    if is_streaming {
        let text = response
            .output
            .first()
            .and_then(|item| item.text_content())
            .unwrap_or_default();
        return Ok((
            [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
            responses_sse_from_text_chunks(&[text.as_str()]),
        )
            .into_response());
    }

    Ok(Json(encode_responses_response(response)).into_response())
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

fn responses_sse_from_text_chunks(chunks: &[&str]) -> String {
    let mut output = String::new();
    for chunk in chunks {
        output.push_str("event: response.output_text.delta\n");
        output.push_str("data: ");
        output.push_str(chunk);
        output.push_str("\n\n");
    }
    output.push_str("event: response.completed\n");
    output.push_str("data: {}\n\n");
    output.push_str("data: [DONE]\n\n");
    output
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::net::TcpListener;

    use super::create_response;
    use super::responses_sse_from_text_chunks;
    use crate::{db, AppState};

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
        assert!(body.contains("data: upstream hello"));
        assert!(body.ends_with("data: [DONE]\n\n"));
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
            State(state),
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
