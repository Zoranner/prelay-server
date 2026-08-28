use super::*;

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
