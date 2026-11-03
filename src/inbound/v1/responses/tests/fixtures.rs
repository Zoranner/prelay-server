use super::*;
use std::sync::Arc;
use tokio::sync::Notify;

mod candidates;
mod rewriting;

pub(super) use candidates::{
    spawn_failing_chat_upstream, spawn_invalid_chat_upstream, spawn_status_chat_upstream,
};
pub(super) use rewriting::spawn_rewriting_native_responses_upstream;

pub(super) fn responses_sse_from_text_chunks(chunks: &[&str]) -> String {
    let mut output = String::new();
    for chunk in chunks {
        output.push_str(
            std::str::from_utf8(&crate::codec::bridge::stream::responses_text_delta_sse(
                chunk,
            ))
            .expect("sse chunk is utf8"),
        );
    }
    output.push_str(
        std::str::from_utf8(&crate::codec::bridge::stream::responses_completed_sse())
            .expect("sse chunk is utf8"),
    );
    output
}

pub(super) async fn spawn_chat_upstream() -> String {
    pub(super) async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
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

pub(super) async fn spawn_streaming_chat_upstream() -> String {
    pub(super) async fn handler(
        Json(payload): Json<serde_json::Value>,
    ) -> axum::response::Response {
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

pub(super) async fn spawn_delayed_streaming_chat_upstream() -> (String, Arc<Notify>) {
    pub(super) async fn handler(
        State(release_tail): State<Arc<Notify>>,
        Json(payload): Json<serde_json::Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;

        assert_eq!(payload["model"], "deepseek-chat");
        assert_eq!(payload["stream"], true);
        let stream =
            futures::stream::unfold((0, release_tail), |(step, release_tail)| async move {
                match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
                        )),
                        (1, release_tail),
                    )),
                    1 => {
                        release_tail.notified().await;
                        Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                              data: [DONE]\n\n",
                            )),
                            (2, release_tail),
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

    let release_tail = Arc::new(Notify::new());
    let app = Router::new()
        .route("/chat/completions", post(handler))
        .with_state(Arc::clone(&release_tail));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let addr = listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    (format!("http://{addr}"), release_tail)
}

pub(super) async fn spawn_native_responses_upstream() -> String {
    pub(super) async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
        assert_eq!(payload["model"], "gpt-4.1");
        assert_eq!(
            payload["input"],
            json!([{ "role": "user", "content": "hello" }])
        );
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

pub(super) async fn spawn_capturing_native_responses_upstream() -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
) {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let state = captured.clone();
    let app = Router::new().route(
        "/responses",
        post(move |Json(payload): Json<serde_json::Value>| {
            let state = state.clone();
            async move {
                state
                    .lock()
                    .expect("capture upstream request")
                    .push(payload);
                Json(json!({
                    "id": "resp_upstream",
                    "object": "response",
                    "status": "completed",
                    "model": "gpt-4.1",
                    "output": [{
                        "type": "message",
                        "id": "msg_upstream",
                        "status": "completed",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "ok", "annotations": []}]
                    }],
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let address = listener.local_addr().expect("upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    (format!("http://{address}"), captured)
}

pub(super) async fn spawn_streaming_native_responses_upstream() -> (String, Arc<Notify>) {
    pub(super) async fn handler(
        State(release_tail): State<Arc<Notify>>,
        Json(payload): Json<serde_json::Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;

        assert_eq!(payload["model"], "gpt-4.1");
        assert_eq!(payload["stream"], true);
        assert_eq!(
            payload["input"],
            json!([{ "role": "user", "content": "hello" }])
        );
        let stream = futures::stream::unfold(
            (0, release_tail),
            |(step, release_tail)| async move {
                match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"event: response.output_text.delta\ndata: hel\n\n",
                        )),
                        (1, release_tail),
                    )),
                    1 => {
                        release_tail.notified().await;
                        Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"event: response.output_text.delta\ndata: lo\n\n\
                              event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_upstream\",\"output\":[{\"type\":\"message\",\"id\":\"msg_upstream\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\",\"annotations\":[]}]}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}}\n\n",
                        )),
                        (2, release_tail),
                    ))
                    }
                    _ => None,
                }
            },
        );
        (
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response()
    }

    let release_tail = Arc::new(Notify::new());
    let app = Router::new()
        .route("/responses", post(handler))
        .with_state(Arc::clone(&release_tail));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let addr = listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    (format!("http://{addr}"), release_tail)
}

pub(super) async fn spawn_history_asserting_chat_upstream() -> String {
    pub(super) async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
        let messages = payload["messages"].as_array().expect("messages");
        let content = match messages.len() {
            1 => {
                assert_eq!(messages[0]["content"], "first user");
                "first assistant"
            }
            3 => {
                assert_eq!(messages[0]["role"], "user");
                assert_eq!(messages[0]["content"], "first user");
                assert_eq!(messages[1]["role"], "assistant");
                assert_eq!(messages[1]["content"], "first assistant");
                assert_eq!(messages[2]["role"], "user");
                assert_eq!(messages[2]["content"], "second user");
                "history accepted"
            }
            5 => {
                assert_eq!(messages[0]["role"], "user");
                assert_eq!(messages[0]["content"], "first user");
                assert_eq!(messages[1]["role"], "assistant");
                assert_eq!(messages[1]["content"], "first assistant");
                assert_eq!(messages[2]["role"], "user");
                assert_eq!(messages[2]["content"], "second user");
                assert_eq!(messages[3]["role"], "assistant");
                assert_eq!(messages[3]["content"], "history accepted");
                assert_eq!(messages[4]["role"], "user");
                assert_eq!(messages[4]["content"], "third user");
                "full history accepted"
            }
            len => panic!("unexpected history length: {len}"),
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

pub(super) async fn spawn_tool_roundtrip_chat_upstream() -> String {
    pub(super) async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
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

pub(super) async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read body");
    serde_json::from_slice(&body).expect("json body")
}
