use super::*;

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

#[tokio::test]
async fn records_failed_response_request_log_when_upstream_fails() {
    let upstream = spawn_failing_chat_upstream().await;
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

    create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect_err("upstream failure should fail");
    let logs = state
        .storage
        .list_request_logs(&auth.access.0.identity_id, 10)
        .await
        .expect("load identity request log totals");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "failed");
}
