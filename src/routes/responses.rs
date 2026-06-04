use axum::{extract::State, routing::post, Json, Router};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    bridge::{
        internal::{InternalContentPart, InternalOutputItem, InternalResponse, InternalRole},
        responses_decode::decode_responses_request,
        responses_encode::encode_responses_response,
    },
    error::AppError,
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/responses", post(create_response))
}

async fn create_response(
    State(_state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let request = decode_responses_request(payload)?;
    let text = request
        .messages
        .last()
        .and_then(|message| message.content.first())
        .map(|content| match content {
            InternalContentPart::Text(text) => text.clone(),
        })
        .unwrap_or_default();
    let response = InternalResponse {
        id: format!("resp_{}", Uuid::new_v4().simple()),
        model: request.model,
        output: vec![InternalOutputItem::Message {
            id: format!("msg_{}", Uuid::new_v4().simple()),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text(text)],
        }],
        usage: None,
    };

    Ok(Json(encode_responses_response(response)))
}

#[cfg(test)]
mod tests {
    use axum::extract::State;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    use super::create_response;
    use crate::{db, AppState};

    #[tokio::test]
    async fn creates_non_streaming_response_from_text_input() {
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
        assert_eq!(response.0["output"][0]["role"], "assistant");
        assert_eq!(response.0["output"][0]["content"][0]["text"], "hello");
    }
}
