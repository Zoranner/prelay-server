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

async fn spawn_failing_chat_upstream() -> String {
    async fn handler() -> axum::response::Response {
        use axum::{http::StatusCode, response::IntoResponse};

        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "message": "primary unavailable" } })),
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
