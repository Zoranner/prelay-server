use super::*;

#[tokio::test]
async fn records_successful_response_activity() {
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
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load identity activity totals");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "success");
    assert_eq!(logs[0].input_tokens, Some(3));
    assert_eq!(logs[0].output_tokens, Some(4));
}

#[tokio::test]
async fn records_failed_response_activity_when_upstream_fails() {
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
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load identity activity totals");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "failed");
}
