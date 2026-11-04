use super::*;

pub(crate) async fn spawn_rich_native_responses_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
        assert_eq!(payload["model"], "gpt-4.1");
        assert_eq!(
            payload["input"],
            json!([{ "role": "user", "content": "hello" }])
        );
        Json(json!({
            "id": "resp_native_rich",
            "object": "response",
            "model": "gpt-4.1",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [
                        { "type": "summary_text", "text": "inspect first" }
                    ],
                    "content": [
                        { "type": "reasoning_text", "text": "detailed reasoning" }
                    ],
                    "encrypted_content": "opaque-reasoning"
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "phase": "final_answer",
                    "content": [
                        { "type": "output_text", "text": "native answer" },
                        { "type": "refusal", "refusal": "cannot comply" }
                    ]
                },
                {
                    "type": "custom_tool_call",
                    "call_id": "call_custom",
                    "name": "apply_patch",
                    "input": "*** Begin Patch"
                }
            ],
            "usage": {
                "input_tokens": 3,
                "output_tokens": 4,
                "reasoning_tokens": 2,
                "input_tokens_details": {
                    "cached_tokens": 1,
                    "cache_write_tokens": 2
                }
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
