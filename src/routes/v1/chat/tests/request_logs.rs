use super::*;

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
