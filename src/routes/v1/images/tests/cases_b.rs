    #[tokio::test]
    async fn records_rate_limit_observability_without_image_or_prompt_content() {
        let upstream = spawn_image_upstream(
            StatusCode::TOO_MANY_REQUESTS,
            Bytes::from_static(
                br#"{"error":{"message":"prompt=private prompt url=https://images.example/private-result b64_json=c2VjcmV0LWltYWdl api_key=sk-secret-image-key token=endpoint-secret-token"}}"#,
            ),
            "application/json",
            Some("cf-ray-image-123"),
        )
        .await;
        let state = test_state().await;
        let provider = test_provider_with_capabilities(
            "Rate limited image provider",
            "custom_image",
            &upstream.url,
            "sk-image",
            Some(&image_capabilities()),
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;
        let identity_id = auth.access.0.identity_id.clone();

        let error = create_image_generation(
            State(state.clone()),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect_err("rate limited image request must fail");
        assert!(format!("{error:?}").contains("429"));

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load failed image log");
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].upstream_request_id.as_deref(),
            Some("cf-ray-image-123")
        );
        assert_eq!(
            logs[0].error_message.as_deref(),
            Some("上游请求失败: 429 Too Many Requests")
        );
        let summary = serde_json::to_string(&logs[0]).expect("serialize request log summary");
        assert!(!summary.contains("private prompt"));
        assert!(!summary.contains("https://images.example/private-result"));
        assert!(!summary.contains("c2VjcmV0LWltYWdl"));
        assert!(!summary.contains("sk-secret-image-key"));
        assert!(!summary.contains("endpoint-secret-token"));
        assert!(!summary.contains("b64_json"));
    }
    #[tokio::test]
    async fn fails_over_from_server_error_to_second_image_candidate() {
        let primary = spawn_image_upstream(
            StatusCode::INTERNAL_SERVER_ERROR,
            Bytes::from_static(br#"{"error":{"message":"primary unavailable"}}"#),
            "application/json",
            None,
        )
        .await;
        let expected_body =
            Bytes::from_static(br#"{"data":[{"url":"https://images.example/result"}]}"#);
        let backup = spawn_image_upstream(
            StatusCode::OK,
            expected_body.clone(),
            "application/json",
            None,
        )
        .await;
        let state = test_state().await;
        let primary_provider = test_provider_with_capabilities(
            "Primary image provider",
            "custom_image",
            &primary.url,
            "sk-primary",
            Some(&image_capabilities()),
        )
        .await
        .expect("create primary provider");
        let backup_provider = test_provider_with_capabilities(
            "Backup image provider",
            "custom_image",
            &backup.url,
            "sk-backup",
            Some(&image_capabilities()),
        )
        .await
        .expect("create backup provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[primary_provider, backup_provider],
            "image-public",
            "image-upstream",
        )
        .await;
        let identity_id = auth.access.0.identity_id.clone();

        let response = create_image_generation(
            State(state.clone()),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect("fall back to backup image provider");

        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read backup response"),
            expected_body
        );
        assert_eq!(primary.hits.load(Ordering::SeqCst), 1);
        assert_eq!(backup.hits.load(Ordering::SeqCst), 1);
        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load failover logs");
        assert_eq!(logs.len(), 2);
        assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
        assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 1);
    }

    #[tokio::test]
    async fn fails_over_when_failed_request_log_cannot_be_written() {
        let primary = spawn_image_upstream(
            StatusCode::INTERNAL_SERVER_ERROR,
            Bytes::from_static(br#"{"error":{"message":"primary unavailable"}}"#),
            "application/json",
            None,
        )
        .await;
        let expected_body = Bytes::from_static(br#"{"data":[{"b64_json":"backup-image-bytes"}]}"#);
        let backup = spawn_image_upstream(
            StatusCode::OK,
            expected_body.clone(),
            "application/json; charset=utf-8",
            None,
        )
        .await;
        let (state, connection) = test_state_with_connection().await;
        let primary_provider = test_provider_with_capabilities(
            "Primary image provider",
            "custom_image",
            &primary.url,
            "sk-primary",
            Some(&image_capabilities()),
        )
        .await
        .expect("create primary provider");
        let backup_provider = test_provider_with_capabilities(
            "Backup image provider",
            "custom_image",
            &backup.url,
            "sk-backup",
            Some(&image_capabilities()),
        )
        .await
        .expect("create backup provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[primary_provider, backup_provider],
            "image-public",
            "image-upstream",
        )
        .await;
        reject_request_log_inserts(&connection).await;

        let response = create_image_generation(
            State(state),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect("request log failure must not block candidate failover");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json; charset=utf-8"))
        );
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read backup response"),
            expected_body
        );
        assert_eq!(primary.hits.load(Ordering::SeqCst), 1);
        assert_eq!(backup.hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn returns_success_bytes_when_request_log_cannot_be_written() {
        let expected_body = Bytes::from_static(
            br#"{"created":1,"data":[{"url":"https://images.example/private-result"}]}"#,
        );
        let upstream = spawn_image_upstream(
            StatusCode::CREATED,
            expected_body.clone(),
            "application/json; charset=utf-8",
            None,
        )
        .await;
        let (state, connection) = test_state_with_connection().await;
        let provider = test_provider_with_capabilities(
            "Image provider",
            "custom_image",
            &upstream.url,
            "sk-image",
            Some(&image_capabilities()),
        )
        .await
        .expect("create image provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;
        reject_request_log_inserts(&connection).await;

        let response = create_image_generation(
            State(state),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect("request log failure must not discard a successful image response");

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json; charset=utf-8"))
        );
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read successful image response"),
            expected_body
        );
    }

    #[tokio::test]
    async fn logs_sanitized_failure_when_upstream_connection_fails() {
        let upstream_url = spawn_connection_failure_upstream().await;
        let mut state = test_state().await;
        state.client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("build direct test client");
        let provider = test_provider_with_capabilities(
            "Image provider",
            "custom_image",
            &upstream_url,
            "sk-private-provider-key",
            Some(&image_capabilities()),
        )
        .await
        .expect("create image provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;
        let identity_id = auth.access.0.identity_id.clone();

        let error = create_image_generation(
            State(state.clone()),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect_err("closed upstream connection must fail");

        match error {
            crate::error::AppError::Upstream { status, message } => {
                assert_eq!(status, None, "unexpected upstream response: {message}");
                assert_eq!(message, "上游连接失败");
            }
            other => panic!("expected upstream error, got {other:?}"),
        }
        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load connection failure request log");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].protocol_in.as_deref(), Some("images_generations"));
        assert_eq!(logs[0].status, "failed");
        assert_eq!(logs[0].error_code.as_deref(), Some("upstream_connection"));
        assert_eq!(logs[0].error_message.as_deref(), Some("上游连接失败"));
        let summary = serde_json::to_string(&logs[0]).expect("serialize request log summary");
        for secret in [
            upstream_url.as_str(),
            "private prompt",
            auth.token.as_str(),
            "sk-private-provider-key",
        ] {
            assert!(!summary.contains(secret));
        }
    }
