use super::*;

#[tokio::test]
async fn forwards_anthropic_messages_request_to_chat_completions_upstream() {
    let upstream = spawn_chat_upstream().await;
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
async fn forwards_anthropic_messages_request_to_responses_upstream_for_openai_provider() {
    let upstream = spawn_responses_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
        .await
        .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;
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
            "model": "gpt-4.1",
            "max_tokens": 1024,
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        }))
        .send()
        .await
        .expect("send request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("parse response json");
    assert_eq!(body["type"], "message");
    assert_eq!(body["role"], "assistant");
    assert_eq!(body["model"], "gpt-4.1");
    assert_eq!(body["content"][0]["type"], "text");
    assert_eq!(body["content"][0]["text"], "responses hello");
    assert_eq!(body["usage"]["input_tokens"], 3);
    assert_eq!(body["usage"]["output_tokens"], 4);

    let log = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 1)
        .await
        .expect("load request log")
        .pop()
        .expect("request log");
    assert_eq!(log.protocol_upstream.as_deref(), Some("responses"));
    assert_eq!(log.input_tokens, Some(3));
    assert_eq!(log.output_tokens, Some(4));

    server.abort();
}

#[tokio::test]
async fn forwards_anthropic_messages_request_to_native_upstream() {
    let upstream = spawn_native_anthropic_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider(
        "claude-sonnet",
        "anthropic_compatible",
        &upstream,
        "sk-upstream",
    )
    .await
    .expect("create provider");
    let auth =
        create_test_endpoint_auth(&state.storage, &provider, "claude-sonnet", "claude-sonnet")
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
            "model": "claude-sonnet",
            "max_tokens": 1024,
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        }))
        .send()
        .await
        .expect("send request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("parse response json");
    assert_eq!(body["id"], "msg_native");
    assert_eq!(body["model"], "claude-sonnet");
    assert_eq!(body["content"][0]["text"], "native hello");

    server.abort();
}
