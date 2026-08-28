#[tokio::test]
async fn rejects_unauthenticated_responses_request() {
    let state = crate::test_support::test_state().await;
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
                .uri("/v1/responses")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn fails_over_responses_to_a_healthy_candidate_and_keeps_using_it() {
    let failing_upstream = spawn_failing_chat_upstream().await;
    let healthy_upstream = spawn_chat_upstream().await;
    let state = crate::test_support::test_state().await;
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
    let payload = json!({ "model": "shared-model", "input": "hello" });

    create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(payload.clone()),
    )
    .await
    .expect("fall back to the healthy candidate");
    create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(payload),
    )
    .await
    .expect("keep using the last successful candidate");

    let logs = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 10)
        .await
        .expect("load request logs");
    assert_eq!(logs.len(), 3);
    assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
    assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
}

#[tokio::test]
async fn rejects_response_when_model_is_not_configured() {
    let state = crate::test_support::test_state().await;
    let auth = create_empty_test_endpoint_auth(&state.storage).await;

    let error = create_response(
        State(state),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect_err("missing endpoint model should fail");

    assert!(format!("{error:?}").contains("接入点未配置支持 responses 的模型 deepseek-chat"));
}

#[tokio::test]
async fn forwards_responses_request_to_chat_completions_upstream() {
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

    let response = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect("create response");

    let response = response_json(response).await;

    assert_eq!(response["object"], "response");
    assert_eq!(response["model"], "deepseek-chat");
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        "upstream hello"
    );
    assert_eq!(response["usage"]["input_tokens"], 3);
    assert_eq!(response["usage"]["output_tokens"], 4);
}

#[tokio::test]
async fn forwards_responses_request_to_chat_bridge_before_anthropic_for_multi_protocol_provider() {
    let upstream = spawn_chat_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider(
        "deepseek-chat",
        "kimi_coding_anthropic",
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
            "input": "hello"
        })),
    )
    .await
    .expect("create response");
    let response = response_json(response).await;

    assert_eq!(response["object"], "response");
    assert_eq!(response["model"], "deepseek-chat");
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        "upstream hello"
    );
}

#[tokio::test]
async fn forwards_responses_request_to_native_upstream() {
    let upstream = spawn_native_responses_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
        .await
        .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;

    let response = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "gpt-4.1",
            "input": "hello"
        })),
    )
    .await
    .expect("create response");
    let response = response_json(response).await;

    assert_eq!(response["id"], "resp_native");
    assert_eq!(response["object"], "response");
    assert_eq!(response["model"], "gpt-4.1");
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        "native response"
    );
}

#[tokio::test]
async fn forwards_non_streaming_responses_request_to_anthropic_messages_upstream() {
    let upstream = spawn_native_anthropic_messages_upstream().await;
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

    let response = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "claude-sonnet",
            "input": "hello"
        })),
    )
    .await
    .expect("create response");
    let response = response_json(response).await;

    assert_eq!(response["object"], "response");
    assert_eq!(response["model"], "claude-sonnet");
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        "anthropic hello"
    );
    assert_eq!(response["usage"]["input_tokens"], 3);
    assert_eq!(response["usage"]["output_tokens"], 4);
}

#[tokio::test]
async fn streams_anthropic_messages_chunks_as_responses_sse() {
    let upstream = spawn_streaming_native_anthropic_messages_upstream().await;
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

    let response = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "claude-sonnet",
            "input": "hello",
            "stream": true
        })),
    )
    .await
    .expect("create response");
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(content_type.starts_with("text/event-stream"));

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read stream body");
    let body = String::from_utf8(body.to_vec()).expect("utf8 body");
    assert!(body.contains("event: response.output_text.delta\ndata: hel\n\n"));
    assert!(body.contains("event: response.output_text.delta\ndata: lo\n\n"));
    assert!(body.contains("event: response.completed"));

    let log = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 1)
        .await
        .expect("load request log")
        .pop()
        .expect("request log");

    assert_eq!(log.protocol_upstream.as_deref(), Some("anthropic_messages"));
    assert_eq!(log.status, "success");
    assert_eq!(log.http_status, Some(200));
    assert_eq!(log.error_code, None);
    assert_eq!(log.error_message, None);
}

#[tokio::test]
async fn records_successful_response_request_log() {
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

    let _response = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect("create response");
    let logs = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 10)
        .await
        .expect("load identity request log totals");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "success");
    assert_eq!(logs[0].input_tokens, Some(3));
    assert_eq!(logs[0].output_tokens, Some(4));
}

#[tokio::test]
async fn records_response_decode_diagnostics_in_request_metadata() {
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

    let _response = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "role": "planner",
                    "content": "hello"
                }
            ]
        })),
    )
    .await
    .expect("create response");
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
    assert_eq!(metadata["diagnostics"][0]["code"], "responses.role.unknown");
    assert_eq!(metadata["diagnostics"][0]["action"], "mapped");
    assert_eq!(metadata["diagnostics"][0]["severity"], "warning");
}
