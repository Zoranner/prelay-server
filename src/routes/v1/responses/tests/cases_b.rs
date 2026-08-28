#[tokio::test]
async fn records_failed_response_request_log_when_upstream_fails() {
    let upstream = spawn_failing_chat_upstream().await;
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

    create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect_err("upstream failure should fail");
    let logs = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 10)
        .await
        .expect("load identity request log totals");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "failed");
}
#[tokio::test]
async fn returns_responses_sse_when_stream_is_true() {
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

    let response = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello",
            "stream": true
        })),
    )
    .await
    .expect("create response")
    .into_response();
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read body");
    let body = String::from_utf8(body.to_vec()).expect("utf8 body");

    assert!(content_type.starts_with("text/event-stream"));
    assert!(body.contains("event: response.output_text.delta"));
    assert!(body.contains("data: hel"));
    assert!(body.contains("data: lo"));
    assert!(body.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn streams_responses_sse_delta_before_upstream_finishes() {
    let upstream = spawn_delayed_streaming_chat_upstream().await;
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

    let started = std::time::Instant::now();
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/responses"))
        .bearer_auth(&auth.token)
        .json(&json!({
            "model": "deepseek-chat",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .expect("send request");
    let mut stream = response.bytes_stream();
    let first = stream
        .next()
        .await
        .expect("first response chunk")
        .expect("first response chunk ok");
    let elapsed = started.elapsed();
    let first = String::from_utf8(first.to_vec()).expect("first chunk utf8");

    assert!(
        elapsed < Duration::from_millis(200),
        "first relay chunk arrived after {elapsed:?}: {first}"
    );
    assert!(first.contains("event: response.output_text.delta"));
    assert!(first.contains("data: hel"));

    server.abort();
}

#[tokio::test]
async fn streams_native_responses_sse_without_waiting_for_upstream_done() {
    let upstream = spawn_streaming_native_responses_upstream().await;
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
        .post(format!("http://{addr}/v1/responses"))
        .bearer_auth(&auth.token)
        .json(&json!({
            "model": "gpt-4.1",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .expect("send request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let started = std::time::Instant::now();
    let mut stream = response.bytes_stream();
    let first = stream
        .next()
        .await
        .expect("first response chunk")
        .expect("first response chunk ok");
    let elapsed = started.elapsed();
    let first = String::from_utf8(first.to_vec()).expect("first chunk utf8");

    assert!(
        elapsed < Duration::from_millis(200),
        "first native response chunk arrived after {elapsed:?}: {first}"
    );
    assert!(first.contains("event: response.output_text.delta"));
    assert!(first.contains("data: hel"));

    stream
        .try_collect::<Vec<_>>()
        .await
        .expect("read remaining response stream");
    let log = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 1)
        .await
        .expect("load request log")
        .pop()
        .expect("request log");
    assert_eq!(log.input_tokens, Some(11));
    assert_eq!(log.output_tokens, Some(7));

    server.abort();
}

#[tokio::test]
async fn prepends_previous_response_messages_to_upstream_chat_request() {
    let upstream = spawn_history_asserting_chat_upstream().await;
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

    let first = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "first user"
        })),
    )
    .await
    .expect("create first response");
    let first = response_json(first).await;
    let first_id = first["id"].as_str().expect("first id");

    let second = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": first_id,
            "input": "second user"
        })),
    )
    .await
    .expect("create second response");
    let second = response_json(second).await;

    assert_eq!(
        second["output"][0]["content"][0]["text"],
        "history accepted"
    );
    let second_id = second["id"].as_str().expect("second id");

    let third = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": second_id,
            "input": "third user"
        })),
    )
    .await
    .expect("create third response");
    let third = response_json(third).await;

    assert_eq!(
        third["output"][0]["content"][0]["text"],
        "full history accepted"
    );
}

#[tokio::test]
async fn bridges_function_tool_call_roundtrip() {
    let upstream = spawn_tool_roundtrip_chat_upstream().await;
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

    let first = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "please read"
        })),
    )
    .await
    .expect("create first response");
    let first = response_json(first).await;
    let first_id = first["id"].as_str().expect("first id");
    assert_eq!(first["output"][0]["type"], "function_call");
    assert_eq!(first["output"][0]["call_id"], "call_1");
    assert_eq!(first["output"][0]["name"], "read_file");

    let second = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": first_id,
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "file text"
                }
            ]
        })),
    )
    .await
    .expect("create second response");
    let second = response_json(second).await;

    assert_eq!(second["output"][0]["content"][0]["text"], "tool accepted");

    let logs = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 10)
        .await
        .expect("load tool call request logs");
    assert_eq!(logs.len(), 2);
    assert!(logs.iter().all(|log| log.status == "success"));
}

#[tokio::test]
async fn rejects_unknown_previous_response_id() {
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

    let error = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": "resp_missing",
            "input": "second user"
        })),
    )
    .await
    .expect_err("unknown previous response id should fail");

    assert!(format!("{error:?}").contains("previous_response_id resp_missing 不存在"));
}

#[test]
fn encodes_upstream_text_chunks_as_responses_sse_events() {
    let encoded = responses_sse_from_text_chunks(&["hel", "lo"]);

    assert!(encoded.contains("event: response.output_text.delta\ndata: hel\n\n"));
    assert!(encoded.contains("event: response.output_text.delta\ndata: lo\n\n"));
    assert!(encoded.contains("event: response.completed\ndata: {}\n\n"));
    assert!(encoded.ends_with("data: [DONE]\n\n"));
}
