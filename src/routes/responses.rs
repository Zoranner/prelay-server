use axum::{extract::State, routing::post, Json, Router};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    bridge::{
        responses_decode::decode_responses_request, responses_encode::encode_responses_response,
    },
    db,
    error::AppError,
    providers::chat_completions::{decode_chat_response, encode_chat_request},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/responses", post(create_response))
}

async fn create_response(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let request = decode_responses_request(payload)?;
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
        return Err(AppError::BadRequest(format!(
            "上游请求失败: {}",
            upstream_response.status()
        )));
    }

    let upstream_json = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| AppError::Internal(error.into()))?;
    let mut response = decode_chat_response(upstream_json)?;
    response.id = format!("resp_{}", Uuid::new_v4().simple());

    Ok(Json(encode_responses_response(response)))
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::net::TcpListener;

    use super::create_response;
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

        assert_eq!(response.0["object"], "response");
        assert_eq!(response.0["model"], "deepseek-chat");
        assert_eq!(
            response.0["output"][0]["content"][0]["text"],
            "upstream hello"
        );
        assert_eq!(response.0["usage"]["input_tokens"], 3);
        assert_eq!(response.0["usage"]["output_tokens"], 4);
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
}
