use super::*;

#[tokio::test]
async fn fails_over_responses_and_keeps_the_session_on_the_healthy_candidate() {
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
    let mut auth = create_test_endpoint_auth_with_candidates(
        &state.storage,
        &[primary, backup],
        "deepseek-chat",
        "deepseek-chat",
    )
    .await;
    auth.access.0.session_id = Some("session-sticky".to_string());
    let primary_id = auth.provider_ids[0].clone();
    let payload = json!({ "model": "deepseek-chat", "input": "hello" });

    create_response(
        State(state.clone()),
        auth.access.clone(),
        V1Json(payload.clone()),
    )
    .await
    .expect("fall back to the healthy candidate");
    state.failover.clear(&primary_id);
    create_response(State(state.clone()), auth.access.clone(), V1Json(payload))
        .await
        .expect("keep the session on the healthy candidate");

    let logs = state
        .storage
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load activities");
    assert_eq!(logs.len(), 3);
    assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
    assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
}

#[tokio::test]
async fn new_response_session_starts_from_the_configured_order() {
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
    let mut auth = create_test_endpoint_auth_with_candidates(
        &state.storage,
        &[primary, backup],
        "deepseek-chat",
        "deepseek-chat",
    )
    .await;
    let primary_id = auth.provider_ids[0].clone();
    auth.access.0.session_id = Some("session-a".to_string());
    let payload = json!({ "model": "deepseek-chat", "input": "hello" });

    create_response(
        State(state.clone()),
        auth.access.clone(),
        V1Json(payload.clone()),
    )
    .await
    .expect("first session falls back to the healthy candidate");
    state.failover.clear(&primary_id);
    auth.access.0.session_id = Some("session-b".to_string());
    create_response(State(state.clone()), auth.access.clone(), V1Json(payload))
        .await
        .expect("new session starts from the configured order");

    let logs = state
        .storage
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load activities");
    assert_eq!(logs.len(), 4);
    assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 2);
    assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
}

#[tokio::test]
async fn fails_over_responses_when_primary_forbids_the_request() {
    let failing_upstream = spawn_status_chat_upstream(axum::http::StatusCode::FORBIDDEN).await;
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
        "deepseek-chat",
        "deepseek-chat",
    )
    .await;

    create_response(
        State(state.clone()),
        auth.access.clone(),
        V1Json(json!({ "model": "deepseek-chat", "input": "hello" })),
    )
    .await
    .expect("forbidden primary should fall back to the healthy candidate");

    let logs = state
        .storage
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load activities");
    let failed = logs
        .iter()
        .find(|activity| activity.status == "failed")
        .expect("record forbidden primary failure");
    assert_eq!(failed.http_status, Some(403));
    assert_eq!(failed.error_code.as_deref(), Some("upstream_status"));
    assert_eq!(
        failed.error_message.as_deref(),
        Some("upstream request failed: status=403 Forbidden; detail=upstream failed")
    );
}

#[tokio::test]
async fn fails_over_responses_when_primary_returns_an_invalid_success_payload() {
    let failing_upstream = spawn_invalid_chat_upstream().await;
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
        "deepseek-chat",
        "deepseek-chat",
    )
    .await;

    create_response(
        State(state),
        auth.access,
        V1Json(json!({ "model": "deepseek-chat", "input": "hello" })),
    )
    .await
    .expect("invalid primary payload should fall back to the healthy candidate");
}

#[tokio::test]
async fn rejects_response_when_model_is_not_configured() {
    let state = crate::test_support::test_state().await;
    let auth = create_empty_test_endpoint_auth(&state.storage).await;

    let error = create_response(
        State(state),
        auth.access.clone(),
        V1Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect_err("missing endpoint model should fail");

    assert!(
        format!("{error:?}").contains("endpoint has no model=deepseek-chat for protocol=responses")
    );
}
