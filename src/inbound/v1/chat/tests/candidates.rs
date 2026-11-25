use super::*;

#[tokio::test]
async fn fails_over_and_keeps_the_session_on_the_healthy_candidate() {
    let failing_upstream = spawn_failing_chat_upstream().await;
    let healthy_upstream = spawn_chat_upstream().await;
    let state = test_state().await;
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
    let identity_id = auth.access.0.identity_id.clone();
    let payload = json!({
        "model": "deepseek-chat",
        "messages": [{ "role": "user", "content": "hello" }]
    });

    let first = create_chat_completion(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(payload.clone()),
    )
    .await
    .expect("fall back to the healthy candidate");
    assert_eq!(response_json(first).await["id"], "chatcmpl_test");
    let first_attempt_logs = state
        .storage
        .list_activities(&identity_id, 10)
        .await
        .expect("load first attempt activities");
    assert_eq!(first_attempt_logs.len(), 2);
    let mut first_attempt_paths = Vec::new();
    for log in &first_attempt_logs {
        first_attempt_paths.push(
            state
                .storage
                .find_activity_rollout(&log.id)
                .await
                .expect("load first attempt rollout")
                .expect("first attempt rollout")
                .rollout_path,
        );
    }
    assert_eq!(first_attempt_paths[0], first_attempt_paths[1]);

    state.failover.clear(&primary_id);
    let second = create_chat_completion(State(state.clone()), auth.access, axum::Json(payload))
        .await
        .expect("keep the session on the healthy candidate");
    assert_eq!(response_json(second).await["id"], "chatcmpl_test");

    let logs = state
        .storage
        .list_activities(&identity_id, 10)
        .await
        .expect("load activities");
    assert_eq!(logs.len(), 3);
    assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
    assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
}

#[tokio::test]
async fn new_session_starts_from_the_configured_order() {
    let failing_upstream = spawn_failing_chat_upstream().await;
    let healthy_upstream = spawn_chat_upstream().await;
    let state = test_state().await;
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
    let payload = json!({
        "model": "deepseek-chat",
        "messages": [{ "role": "user", "content": "hello" }]
    });

    create_chat_completion(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(payload.clone()),
    )
    .await
    .expect("first session falls back to the healthy candidate");
    state.failover.clear(&primary_id);
    auth.access.0.session_id = Some("session-b".to_string());
    create_chat_completion(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(payload),
    )
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
