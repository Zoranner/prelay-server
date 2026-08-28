use super::*;

#[tokio::test]
async fn bridges_chat_tool_call_to_anthropic_tool_use() {
    let upstream = spawn_tool_call_chat_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider(
        "deepseek-chat",
        "openai_compatible",
        &upstream,
        "sk-upstream",
    )
    .await
    .expect("create provider");
    let auth =
        create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
            .await;
    let app = Router::new().nest(
        "/v1",
        super::router()
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(
                state.clone(),
                crate::routes::v1::auth::require_protocol_auth,
            )),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("read test server address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/messages"))
        .bearer_auth(&auth.token)
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
    let state = crate::test_support::test_state().await;
    let provider = test_provider(
        "deepseek-chat",
        "openai_compatible",
        &upstream,
        "sk-upstream",
    )
    .await
    .expect("create provider");
    let auth =
        create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
            .await;
    let app = Router::new().nest(
        "/v1",
        super::router()
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(
                state.clone(),
                crate::routes::v1::auth::require_protocol_auth,
            )),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("read test server address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/messages"))
        .bearer_auth(&auth.token)
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
