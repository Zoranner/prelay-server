    #[tokio::test]
    async fn records_successful_chat_completion_request_log() {
        let upstream = spawn_chat_upstream().await;
        let state = test_state().await;
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
        let identity_id = auth.access.0.identity_id.clone();

        let _response = create_chat_completion(
            State(state.clone()),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");
        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load identity request log totals");

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, "success");
        assert_eq!(logs[0].input_tokens, Some(3));
        assert_eq!(logs[0].output_tokens, Some(4));
        assert_eq!(logs[0].endpoint_name.as_deref(), Some("Test Endpoint"));
    }

    #[tokio::test]
    async fn forwards_chat_completion_to_protocol_specific_base_url() {
        let default_upstream = spawn_unexpected_chat_upstream().await;
        let chat_upstream = spawn_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider_with_capabilities(
            "deepseek-chat",
            "openai_compatible",
            &default_upstream,
            "sk-upstream",
            Some(&ProviderCapabilityOverrides {
                protocol_base_urls: Some(crate::models::ProviderProtocolBaseUrls {
                    responses: None,
                    openai: Some(chat_upstream),
                    anthropic: None,
                    ..Default::default()
                }),
                ..ProviderCapabilityOverrides::default()
            }),
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
                .await;

        let response = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");
        let body = response_json(response).await;

        assert_eq!(
            body["choices"][0]["message"]["content"],
            "chat upstream hello"
        );
    }

    #[tokio::test]
    async fn records_successful_chat_completion_upstream_request_id() {
        let upstream = spawn_request_id_chat_upstream().await;
        let state = test_state().await;
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
        let identity_id = auth.access.0.identity_id.clone();

        let _response = create_chat_completion(
            State(state.clone()),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");

        let upstream_request_id = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load upstream request id")[0]
            .upstream_request_id
            .clone();

        assert_eq!(upstream_request_id.as_deref(), Some("req_chat_123"));
    }

    #[tokio::test]
    async fn records_failed_chat_completion_upstream_request_id_and_error_message() {
        let upstream = spawn_error_chat_upstream().await;
        let state = test_state().await;
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
        let identity_id = auth.access.0.identity_id.clone();

        let error = create_chat_completion(
            State(state.clone()),
            auth.access,
            axum::Json(json!({
                "model": "deepseek-chat",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect_err("upstream error should fail");

        assert!(format!("{error:?}").contains("上游请求失败"));
        let row = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load failed request log");

        assert_eq!(row[0].upstream_request_id.as_deref(), Some("cf-ray-123"));
        assert_eq!(row[0].error_message.as_deref(), Some("provider overloaded"));
    }

    #[tokio::test]
    async fn streams_chat_completion_without_waiting_for_upstream_done() {
        let upstream = spawn_streaming_chat_upstream().await;
        let state = test_state().await;
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
        let identity_id = auth.access.0.identity_id.clone();
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
            .post(format!("http://{addr}/v1/chat/completions"))
            .bearer_auth(&auth.token)
            .json(&json!({
                "model": "deepseek-chat",
                "stream": true,
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            }))
            .send()
            .await
            .expect("send request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .expect("stream response content type");
        assert!(
            content_type.starts_with("text/event-stream"),
            "unexpected content type: {content_type}"
        );

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
            "first chat completion chunk arrived after {elapsed:?}: {first}"
        );
        assert!(first.contains("data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}"));

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load stream request log");
        assert_eq!(logs.len(), 1);
        let first_token_ms = logs[0].first_token_ms;
        assert!(
            first_token_ms.is_some(),
            "first_token_ms should be recorded after the first stream chunk"
        );

        server.abort();
    }
