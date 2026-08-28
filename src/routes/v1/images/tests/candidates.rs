use super::*;

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
