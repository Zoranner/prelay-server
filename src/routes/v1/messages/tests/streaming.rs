use super::*;

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
        .list_activities(&auth.access.0.identity_id, 1)
        .await
        .expect("load activity")
        .pop()
        .expect("activity");

    assert_eq!(log.protocol_upstream.as_deref(), Some("responses"));
    assert_eq!(log.status, "success");
    assert_eq!(log.http_status, Some(200));
    assert!(log.first_token_ms.is_some());

    server.abort();
}

#[tokio::test]
async fn streams_chat_completions_text_delta_without_waiting_for_upstream_done() {
    let upstream = spawn_streaming_chat_upstream().await;
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

    server.abort();
}

#[tokio::test]
async fn streams_native_anthropic_messages_sse_without_waiting_for_upstream_done() {
    let upstream = spawn_streaming_native_anthropic_upstream().await;
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

    server.abort();
}
