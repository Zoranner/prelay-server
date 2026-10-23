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
    let rollout = state
        .storage
        .find_activity_rollout(&logs[0].id)
        .await
        .expect("load response rollout")
        .expect("response rollout exists");
    let events = state
        .activity_store
        .rollouts()
        .read_events(&rollout.rollout_path)
        .expect("read response rollout");
    assert!(events.iter().any(|event| {
        event.event_type == "message"
            && event.payload["phase"] == json!("request")
            && event.payload["content"] == json!("hello")
    }));
    assert!(events.iter().any(|event| {
        event.event_type == "message"
            && event.payload["phase"] == json!("response")
            && event.payload["content"] == json!("upstream hello")
    }));
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

    let error = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect_err("upstream failure should fail");
    assert!(
        format!("{error:?}").contains("上游请求失败: 500 Internal Server Error: upstream failed"),
        "client-facing error should carry the upstream reason: {error:?}"
    );
    let logs = state
        .storage
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load identity activity totals");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "failed");
    assert_eq!(
        logs[0].error_message.as_deref(),
        Some("上游请求失败: 500 Internal Server Error: upstream failed")
    );
    let rollout = state
        .storage
        .find_activity_rollout(&logs[0].id)
        .await
        .expect("load failed response rollout")
        .expect("failed response rollout exists");
    let events = state
        .activity_store
        .rollouts()
        .read_events(&rollout.rollout_path)
        .expect("read failed response rollout");
    assert!(events.iter().any(|event| event.event_type == "error"));
}
