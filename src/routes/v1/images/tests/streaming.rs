use super::*;

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
        .list_activities(&identity_id, 10)
        .await
        .expect("load streaming image log");
    assert_eq!(logs.len(), 1);
    assert!(logs[0].first_token_ms.is_some());
    let summary = serde_json::to_string(&logs[0]).expect("serialize activity summary");
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
        .list_activities(&identity_id, 10)
        .await
        .expect("load interrupted stream log");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "failed");
    assert_eq!(logs[0].http_status, Some(502));
    assert_eq!(logs[0].error_code.as_deref(), Some("stream_error"));

    server.abort();
}
