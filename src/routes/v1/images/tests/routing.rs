use super::*;

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
        .list_activities(&identity_id, 10)
        .await
        .expect("load image activity");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].protocol_in.as_deref(), Some("images_generations"));
    assert_eq!(
        logs[0].protocol_upstream.as_deref(),
        Some("images_generations")
    );
    assert_eq!(logs[0].input_tokens, None);
    assert_eq!(logs[0].output_tokens, None);
    let summary = serde_json::to_string(&logs[0]).expect("serialize activity summary");
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
