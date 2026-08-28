fn responses_sse_from_text_chunks(chunks: &[&str]) -> String {
    let mut output = String::new();
    for chunk in chunks {
        output.push_str(
            std::str::from_utf8(&crate::bridge::stream::responses_text_delta_sse(chunk))
                .expect("sse chunk is utf8"),
        );
    }
    output.push_str(
        std::str::from_utf8(&crate::bridge::stream::responses_completed_sse())
            .expect("sse chunk is utf8"),
    );
    output
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

async fn spawn_streaming_chat_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
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

async fn spawn_delayed_streaming_chat_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
        use axum::response::IntoResponse;

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

async fn spawn_native_responses_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
        assert_eq!(payload["model"], "gpt-4.1");
        assert_eq!(payload["input"], "hello");
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

async fn spawn_streaming_native_responses_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
        use axum::response::IntoResponse;

        assert_eq!(payload["model"], "gpt-4.1");
        assert_eq!(payload["stream"], true);
        let stream = futures::stream::unfold(0, |step| async move {
            match step {
                0 => Some((
                    Ok::<_, Infallible>(Bytes::from_static(
                        b"event: response.output_text.delta\ndata: hel\n\n",
                    )),
                    1,
                )),
                1 => {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"event: response.output_text.delta\ndata: lo\n\n\
                                  event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}}\n\n",
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

async fn spawn_native_anthropic_messages_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
        assert_eq!(payload["model"], "claude-sonnet");
        assert_eq!(payload["stream"], false);
        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(payload["messages"][0]["content"], "hello");
        Json(json!({
            "id": "msg_native",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet",
            "content": [
                { "type": "text", "text": "anthropic hello" }
            ],
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

async fn spawn_streaming_native_anthropic_messages_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> axum::response::Response {
        use axum::response::IntoResponse;

        assert_eq!(payload["model"], "claude-sonnet");
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(payload["messages"][0]["content"], "hello");
        let stream = futures::stream::unfold(0, |step| async move {
            match step {
                    0 => Some((
                        Ok::<_, Infallible>(Bytes::from_static(
                            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_native_stream\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
                        )),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        Some((
                            Ok::<_, Infallible>(Bytes::from_static(
                                b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
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

async fn spawn_history_asserting_chat_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
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

async fn spawn_tool_roundtrip_chat_upstream() -> String {
    async fn handler(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
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
                            "reasoning_content": "Need to inspect the file before answering.",
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
        assert_eq!(
            messages[1]["reasoning_content"],
            "Need to inspect the file before answering."
        );
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
