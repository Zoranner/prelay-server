    #[tokio::test]
    async fn records_anthropic_decode_diagnostics_in_request_metadata() {
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
                    { "role": "planner", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let metadata_json = state
            .storage
            .list_request_logs(&auth.access.0.identity_id, 1)
            .await
            .expect("load metadata")
            .pop()
            .and_then(|log| log.metadata_json);
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata_json.expect("metadata json")).expect("parse metadata");

        assert_eq!(metadata["schema"], "provider-relay.request_metadata.v2");
        assert_eq!(metadata["diagnostics"][0]["code"], "anthropic.role.unknown");
        assert_eq!(metadata["diagnostics"][0]["severity"], "warning");

        server.abort();
    }
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
