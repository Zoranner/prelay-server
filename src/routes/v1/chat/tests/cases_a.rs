    #[tokio::test]
    async fn rejects_unauthenticated_chat_completion_request() {
        let state = test_state().await;
        let app = Router::new().nest(
            "/v1",
            super::router()
                .with_state(state.clone())
                .layer(middleware::from_fn_with_state(
                    state,
                    crate::routes::v1::auth::require_protocol_auth,
                )),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("route request");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn forwards_chat_completion_request_to_configured_upstream() {
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

        let response = response_json(response).await;

        assert_eq!(response["id"], "chatcmpl_test");
        assert_eq!(
            response["choices"][0]["message"]["content"],
            "chat upstream hello"
        );
    }

    #[tokio::test]
    async fn fails_over_to_a_healthy_candidate_and_keeps_using_it() {
        let failing_upstream = spawn_failing_chat_upstream().await;
        let healthy_upstream = spawn_chat_upstream().await;
        let state = test_state().await;
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
        let identity_id = auth.access.0.identity_id.clone();
        let payload = json!({
            "model": "shared-model",
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let first = create_chat_completion(
            State(state.clone()),
            auth.access.clone(),
            axum::Json(payload.clone()),
        )
        .await
        .expect("fall back to the healthy candidate");
        assert_eq!(response_json(first).await["id"], "chatcmpl_test");

        let second = create_chat_completion(State(state.clone()), auth.access, axum::Json(payload))
            .await
            .expect("keep using the last successful candidate");
        assert_eq!(response_json(second).await["id"], "chatcmpl_test");

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load request logs");
        assert_eq!(logs.len(), 3);
        assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
        assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
    }

    #[tokio::test]
    async fn resolves_endpoint_model_name_to_upstream_model() {
        let upstream = spawn_alias_chat_upstream().await;
        let state = test_state().await;
        let provider = test_provider(
            "DeepSeek Provider",
            "openai_compatible",
            &upstream,
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "coder", "deepseek-chat").await;

        let response = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "coder",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect("create chat completion");

        let response = response_json(response).await;

        assert_eq!(response["model"], "deepseek-chat");
    }

    #[tokio::test]
    async fn rejects_chat_when_provider_only_supports_responses_upstream_protocol() {
        let state = test_state().await;
        let provider = test_provider(
            "DeepSeek Provider",
            "openai",
            "http://127.0.0.1:1",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "coder", "deepseek-chat").await;

        let error = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "coder",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect_err("unsupported provider protocol should fail before upstream");

        assert!(format!("{error:?}").contains("接入点未配置支持 chat_completions 的模型 coder"));
    }

    #[tokio::test]
    async fn rejects_chat_when_provider_only_supports_anthropic_messages_upstream_protocol() {
        let state = test_state().await;
        let provider = test_provider(
            "Claude",
            "anthropic_compatible",
            "http://127.0.0.1:1",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "Claude", "claude-sonnet-4").await;

        let error = create_chat_completion(
            State(state),
            auth.access,
            axum::Json(json!({
                "model": "Claude",
                "messages": [
                    { "role": "user", "content": "hello" }
                ]
            })),
        )
        .await
        .expect_err("anthropic provider should not be exposed as chat completions");

        assert!(format!("{error:?}").contains("接入点未配置支持 chat_completions 的模型 Claude"));
    }
