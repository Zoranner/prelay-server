    #[tokio::test]
    async fn forwards_only_to_image_generation_candidate_and_preserves_json_bytes() {
        let unexpected = spawn_image_upstream(
            StatusCode::OK,
            Bytes::from_static(br#"{"unexpected":true}"#),
            "application/json",
            None,
        )
        .await;
        let expected_body = Bytes::from_static(
            br#"{"created":1, "data":[{"b64_json":"aGVsbG8="},{"url":"https://images.example/private-result"}]}"#,
        );
        let image = spawn_image_upstream(
            StatusCode::OK,
            expected_body.clone(),
            "application/json; charset=utf-8",
            None,
        )
        .await;
        let state = test_state().await;
        let openai = test_provider("OpenAI only", "openai", &unexpected.url, "sk-unexpected")
            .await
            .expect("create OpenAI provider");
        let image_provider = test_provider_with_capabilities(
            "Image provider",
            "custom_image",
            &image.url,
            "sk-image",
            Some(&image_capabilities()),
        )
        .await
        .expect("create image provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[openai, image_provider],
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
        .expect("create image generation");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json; charset=utf-8"))
        );
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read response body"),
            expected_body
        );
        assert_eq!(unexpected.hits.load(Ordering::SeqCst), 0);
        assert_eq!(image.hits.load(Ordering::SeqCst), 1);
        {
            let payloads = image.payloads.lock().expect("lock image payloads");
            assert_eq!(
                payloads.as_slice(),
                &[json!({
                    "model": "image-upstream",
                    "prompt": "private prompt"
                })]
            );
        }

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load image request log");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].protocol_in.as_deref(), Some("images_generations"));
        assert_eq!(
            logs[0].protocol_upstream.as_deref(),
            Some("images_generations")
        );
        assert_eq!(logs[0].input_tokens, None);
        assert_eq!(logs[0].output_tokens, None);
        let summary = serde_json::to_string(&logs[0]).expect("serialize request log summary");
        assert!(!summary.contains("private prompt"));
        assert!(!summary.contains("aGVsbG8="));
        assert!(!summary.contains("https://images.example/private-result"));
    }
    #[tokio::test]
    async fn rejects_image_generation_without_image_protocol_candidate() {
        let state = test_state().await;
        let provider = test_provider("OpenAI only", "openai", "http://127.0.0.1:1", "sk-upstream")
            .await
            .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;

        let error = create_image_generation(
            State(state),
            auth.access,
            Json(json!({
                "model": "image-public",
                "prompt": "private prompt"
            })),
        )
        .await
        .expect_err("provider without image protocol must be rejected");

        assert!(format!("{error:?}").contains("images_generations"));
    }

    #[tokio::test]
    async fn streams_image_events_without_waiting_for_upstream_done() {
        let (upstream, release_second_event) = spawn_streaming_image_upstream().await;
        let state = test_state().await;
        let provider = test_provider_with_capabilities(
            "Streaming image provider",
            "custom_image",
            &upstream,
            "sk-image",
            Some(&image_capabilities()),
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;
        let identity_id = auth.access.0.identity_id.clone();
        let (relay_url, server) = spawn_image_relay(state.clone()).await;

        let response = reqwest::Client::new()
            .post(format!("{relay_url}/v1/images/generations"))
            .bearer_auth(&auth.token)
            .json(&json!({
                "model": "image-public",
                "prompt": "private prompt",
                "stream": true,
                "partial_images": 2
            }))
            .send()
            .await
            .expect("send image request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream; charset=utf-8")
        );

        let mut stream = response.bytes_stream();
        let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("receive first image event before releasing upstream")
            .expect("first image event")
            .expect("first image event bytes");
        let first_event = Bytes::from_static(
            b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aGVs\"}\n\n",
        );
        assert_eq!(first, first_event);

        release_second_event.notify_one();
        let mut complete_stream = first.to_vec();
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(chunk) = stream.next().await {
                complete_stream.extend_from_slice(&chunk.expect("remaining image event bytes"));
            }
        })
        .await
        .expect("receive second image event and EOF after releasing upstream");
        assert_eq!(
            complete_stream,
            b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aGVs\"}\n\ndata: {\"type\":\"image_generation.completed\"}\n\n"
        );

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load streaming image log");
        assert_eq!(logs.len(), 1);
        assert!(logs[0].first_token_ms.is_some());
        let summary = serde_json::to_string(&logs[0]).expect("serialize request log summary");
        assert!(!summary.contains("private prompt"));
        assert!(!summary.contains("aGVs"));

        server.abort();
    }
    #[tokio::test]
    async fn marks_image_request_failed_when_upstream_stream_is_interrupted() {
        let upstream = spawn_interrupted_image_upstream().await;
        let state = test_state().await;
        let provider = test_provider_with_capabilities(
            "Interrupted image provider",
            "custom_image",
            &upstream,
            "sk-image",
            Some(&image_capabilities()),
        )
        .await
        .expect("create provider");
        let auth =
            create_test_endpoint_auth(&state.storage, &provider, "image-public", "image-upstream")
                .await;
        let identity_id = auth.access.0.identity_id.clone();
        let (relay_url, server) = spawn_image_relay(state.clone()).await;

        let response = reqwest::Client::new()
            .post(format!("{relay_url}/v1/images/generations"))
            .bearer_auth(&auth.token)
            .json(&json!({
                "model": "image-public",
                "prompt": "private prompt",
                "stream": true,
                "partial_images": 2
            }))
            .send()
            .await
            .expect("send interrupted image request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let mut stream = response.bytes_stream();
        let first = stream
            .next()
            .await
            .expect("first interrupted image event")
            .expect("first interrupted image event bytes");
        assert_eq!(
            first,
            Bytes::from_static(
                b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aGVs\"}\n\n"
            )
        );
        let stream_error = stream
            .next()
            .await
            .expect("upstream interruption should reach downstream");
        assert!(stream_error.is_err());

        let logs = state
            .storage
            .list_request_logs(&identity_id, 10)
            .await
            .expect("load interrupted stream log");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, "failed");
        assert_eq!(logs[0].http_status, Some(502));
        assert_eq!(logs[0].error_code.as_deref(), Some("stream_error"));

        server.abort();
    }
