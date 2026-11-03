use super::*;

fn assert_previous_response_not_found(error: crate::error::AppError) {
    assert!(matches!(
        error,
        crate::error::AppError::OpenAi {
            code: "previous_response_not_found",
            param: Some("previous_response_id"),
            ..
        }
    ));
}

#[tokio::test]
async fn native_non_streaming_returns_a_local_response_id() {
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
            "input": "hello",
            "store": true
        })),
    )
    .await
    .expect("create response");
    let body = response_json(response).await;
    let id = body["id"].as_str().expect("response id");

    assert!(id.starts_with("resp_"));
    assert_ne!(id, "resp_native");
}

#[tokio::test]
async fn previous_response_replays_exact_items_and_removes_upstream_state_fields() {
    let (upstream, captured) = spawn_capturing_native_responses_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
        .await
        .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;

    let first = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "gpt-4.1",
            "input": "first",
            "store": true
        })),
    )
    .await
    .expect("create first response");
    let first = response_json(first).await;
    let parent_id = first["id"].as_str().expect("parent id");

    let second = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "gpt-4.1",
            "previous_response_id": parent_id,
            "instructions": "new instructions",
            "input": "second",
            "store": true
        })),
    )
    .await
    .expect("create second response");
    let second = response_json(second).await;
    assert!(second["id"]
        .as_str()
        .expect("second id")
        .starts_with("resp_"));

    let requests = captured.lock().expect("lock captured requests");
    let upstream_request = requests.last().expect("second upstream request");
    assert_eq!(upstream_request["input"][0]["content"], "first");
    assert_eq!(upstream_request["input"][1]["role"], "assistant");
    assert_eq!(upstream_request["input"][2]["content"], "second");
    assert!(upstream_request.get("previous_response_id").is_none());
    assert!(upstream_request.get("conversation").is_none());
    assert_eq!(upstream_request["store"], false);
    assert_eq!(upstream_request["instructions"], "new instructions");
}

#[tokio::test]
async fn legacy_previous_response_is_rejected_without_leaking_state() {
    let upstream = spawn_native_responses_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
        .await
        .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;
    let identity_id = auth.access.0.identity_id.clone();

    state
        .storage
        .save_response_session(crate::store::storage::ResponseSessionInsert {
            identity_id: &identity_id,
            response_id: "resp_legacy",
            previous_response_id: None,
            conversation_id: "conv_legacy",
            provider_id: &provider.id,
            model: "gpt-4.1",
            input_messages: &[],
            response: &crate::codec::internal::InternalResponse {
                id: "resp_legacy".to_string(),
                model: "gpt-4.1".to_string(),
                output: Vec::new(),
                usage: None,
            },
        })
        .await
        .expect("save legacy response");

    let error = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "gpt-4.1",
            "previous_response_id": "resp_legacy",
            "input": "again"
        })),
    )
    .await
    .expect_err("legacy response cannot be continued");

    assert_previous_response_not_found(error);
}

#[tokio::test]
async fn previous_response_cannot_cross_identities() {
    let (upstream, captured) = spawn_capturing_native_responses_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider("gpt-4.1", "openai", &upstream, "sk-upstream")
        .await
        .expect("create provider");
    let first_auth =
        create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;
    let second_auth =
        create_test_endpoint_auth(&state.storage, &provider, "gpt-4.1", "gpt-4.1").await;

    let first = create_response(
        State(state.clone()),
        first_auth.access,
        axum::Json(json!({
            "model": "gpt-4.1",
            "input": "first",
            "store": true
        })),
    )
    .await
    .expect("create first response");
    let first = response_json(first).await;
    let parent_id = first["id"].as_str().expect("parent id");

    let error = create_response(
        State(state),
        second_auth.access,
        axum::Json(json!({
            "model": "gpt-4.1",
            "previous_response_id": parent_id,
            "input": "second",
            "store": true
        })),
    )
    .await
    .expect_err("another identity must not continue the response");

    assert_previous_response_not_found(error);
    assert_eq!(captured.lock().expect("lock captured requests").len(), 1);
}

#[tokio::test]
async fn prepends_previous_response_messages_to_upstream_chat_request() {
    let upstream = spawn_history_asserting_chat_upstream().await;
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

    let first = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "first user"
        })),
    )
    .await
    .expect("create first response");
    let first = response_json(first).await;
    let first_id = first["id"].as_str().expect("first id");
    assert!(first_id.starts_with("resp_"));

    let second = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": first_id,
            "input": "second user"
        })),
    )
    .await
    .expect("create second response");
    let second = response_json(second).await;

    assert_eq!(
        second["output"][0]["content"][0]["text"],
        "history accepted"
    );
    let second_id = second["id"].as_str().expect("second id");

    let third = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": second_id,
            "input": "third user"
        })),
    )
    .await
    .expect("create third response");
    let third = response_json(third).await;

    assert_eq!(
        third["output"][0]["content"][0]["text"],
        "full history accepted"
    );

    let logs = state
        .storage
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load response activities");
    assert_eq!(logs.len(), 3);
    let mut conversation_ids = Vec::new();
    for log in &logs {
        let rollout = state
            .storage
            .find_activity_rollout(&log.id)
            .await
            .expect("load response rollout")
            .expect("response rollout exists");
        conversation_ids.push(rollout.thread_id);
    }
    assert!(conversation_ids.iter().all(|id| id.starts_with("conv_")));
    assert!(conversation_ids.windows(2).all(|pair| pair[0] == pair[1]));
}

#[tokio::test]
async fn bridges_function_tool_call_roundtrip() {
    let upstream = spawn_tool_roundtrip_chat_upstream().await;
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

    let first = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "please read"
        })),
    )
    .await
    .expect("create first response");
    let first = response_json(first).await;
    let first_id = first["id"].as_str().expect("first id");
    assert_eq!(first["output"][0]["type"], "function_call");
    assert_eq!(first["output"][0]["call_id"], "call_1");
    assert_eq!(first["output"][0]["name"], "read_file");

    let second = create_response(
        State(state.clone()),
        auth.access.clone(),
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": first_id,
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "file text"
                }
            ]
        })),
    )
    .await
    .expect("create second response");
    let second = response_json(second).await;

    assert_eq!(second["output"][0]["content"][0]["text"], "tool accepted");

    let logs = state
        .storage
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load tool call activities");
    assert_eq!(logs.len(), 2);
    assert!(logs.iter().all(|log| log.status == "success"));
}

#[tokio::test]
async fn rejects_unknown_previous_response_id() {
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

    let error = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "previous_response_id": "resp_missing",
            "input": "second user"
        })),
    )
    .await
    .expect_err("unknown previous response id should fail");

    assert_previous_response_not_found(error);
}
