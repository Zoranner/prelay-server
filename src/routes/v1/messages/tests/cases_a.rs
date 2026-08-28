#[tokio::test]
async fn rejects_unauthenticated_anthropic_messages_request() {
    let state = crate::test_support::test_state().await;
    let app = Router::new().nest(
        "/v1",
        super::router()
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(
                state.clone(),
                crate::routes::v1::auth::require_protocol_auth,
            )),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
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
async fn fails_over_messages_to_a_healthy_candidate_and_keeps_using_it() {
    let failing_upstream = spawn_failing_chat_upstream().await;
    let healthy_upstream = spawn_user_only_chat_upstream().await;
    let state = crate::test_support::test_state().await;
    let primary = test_provider(
        "primary",
        "openai_compatible",
        &failing_upstream,
        "sk-primary",
    )
    .await
    .expect("create primary provider");
    let backup = test_provider(
        "backup",
        "openai_compatible",
        &healthy_upstream,
        "sk-backup",
    )
    .await
    .expect("create backup provider");
    let auth = create_test_endpoint_auth_with_candidates(
        &state.storage,
        &[primary, backup],
        "shared-model",
        "deepseek-chat",
    )
    .await;
    let payload = json!({
        "model": "shared-model",
        "max_tokens": 1024,
        "messages": [{ "role": "user", "content": "hello" }]
    });

    create_message(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(payload.clone()),
    )
    .await
    .expect("fall back to the healthy candidate");
    create_message(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(payload),
    )
    .await
    .expect("keep using the last successful candidate");

    let logs = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 10)
        .await
        .expect("load request logs");
    assert_eq!(logs.len(), 3);
    assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
    assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
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
async fn streams_responses_sse_as_anthropic_messages_sse() {
    let upstream = spawn_streaming_responses_upstream().await;
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
            "stream": true,
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        }))
        .send()
        .await
        .expect("send request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let started_at = std::time::Instant::now();
    let mut stream = response.bytes_stream();
    let first_chunk = stream
        .next()
        .await
        .expect("receive first stream chunk")
        .expect("first stream chunk ok");
    let first_chunk_elapsed = started_at.elapsed();
    let first_chunk = String::from_utf8(first_chunk.to_vec()).expect("utf8 stream chunk");

    assert!(
        first_chunk_elapsed < std::time::Duration::from_millis(200),
        "first chunk took {first_chunk_elapsed:?}"
    );
    assert!(first_chunk.contains("event: content_block_delta"));
    assert!(first_chunk.contains("hel"));

    let body = stream
        .map(|chunk| {
            chunk.map(|chunk| String::from_utf8(chunk.to_vec()).expect("utf8 stream chunk"))
        })
        .try_collect::<String>()
        .await
        .expect("read remaining stream");
    let body = format!("{first_chunk}{body}");
    assert!(body.contains("event: content_block_delta"));
    assert!(body.contains("lo"));
    assert!(body.contains("event: message_stop"));

    let log = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 1)
        .await
        .expect("load request log")
        .pop()
        .expect("request log");

    assert_eq!(log.protocol_upstream.as_deref(), Some("responses"));
    assert_eq!(log.status, "success");
    assert_eq!(log.http_status, Some(200));
    assert!(log.first_token_ms.is_some());

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

#[tokio::test]
async fn records_successful_anthropic_messages_request_log() {
    let upstream = spawn_user_only_chat_upstream().await;
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
                { "role": "user", "content": "hello" }
            ]
        }))
        .send()
        .await
        .expect("send request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let logs = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 10)
        .await
        .expect("load identity request log totals");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "success");
    assert_eq!(logs[0].input_tokens, Some(3));
    assert_eq!(logs[0].output_tokens, Some(4));

    server.abort();
}
