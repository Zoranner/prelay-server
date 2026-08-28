use super::*;

#[tokio::test]
async fn fails_over_to_a_healthy_candidate_and_keeps_using_it() {
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
    let auth = create_test_endpoint_auth_with_candidates(
        &state.storage,
        &[primary, backup],
        "shared-model",
        "deepseek-chat",
    )
    .await;
    let identity_id = auth.access.0.identity_id.clone();
    let payload = json!({
        "model": "shared-model",
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

    let second = create_chat_completion(State(state.clone()), auth.access, axum::Json(payload))
        .await
        .expect("keep using the last successful candidate");
    assert_eq!(response_json(second).await["id"], "chatcmpl_test");

    let logs = state
        .storage
        .list_request_logs(&identity_id, 10)
        .await
        .expect("load request logs");
    assert_eq!(logs.len(), 3);
    assert_eq!(logs.iter().filter(|log| log.status == "failed").count(), 1);
    assert_eq!(logs.iter().filter(|log| log.status == "success").count(), 2);
}
