use super::*;

#[tokio::test]
async fn reuses_one_thread_file_across_response_activities() {
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
    let mut auth =
        create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
            .await;
    auth.access.0.thread_id = Some("thread-shared".to_string());

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
    let first_id = first["id"].as_str().expect("first response id");

    create_response(
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

    let logs = state
        .storage
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load response activities");
    assert_eq!(logs.len(), 2);
    let rollouts = futures::future::join_all(
        logs.iter()
            .map(|log| state.storage.find_activity_rollout(&log.id)),
    )
    .await
    .into_iter()
    .map(|result| result.expect("load rollout").expect("rollout exists"))
    .collect::<Vec<_>>();
    let expected_path = format!("{}/thread-shared.jsonl", auth.access.0.identity_id);
    assert_eq!(rollouts[0].rollout_path, expected_path);
    assert_eq!(rollouts[0].rollout_path, rollouts[1].rollout_path);
    assert_eq!(rollouts[0].thread_id, "thread-shared");
    assert_ne!(rollouts[0].start_line, rollouts[1].start_line);

    let file = state
        .activity_store
        .rollouts()
        .read_verified(&rollouts[0].rollout_path)
        .expect("read shared thread");
    assert_eq!(file.format, crate::domain::activity::ConversationFormat::V1);
    assert_eq!(
        file.meta.as_ref().map(|meta| meta.thread_id.as_str()),
        Some("thread-shared")
    );
    let events = &file.events;
    for content in [
        "first user",
        "first assistant",
        "second user",
        "history accepted",
    ] {
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event.event_type == "message" && event.payload["content"] == json!(content)
                })
                .count(),
            1,
            "expected exactly one event for {content}"
        );
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "turn")
            .count(),
        2
    );
    let contexts = events
        .iter()
        .filter(|event| event.event_type == "context")
        .map(|event| {
            event.payload["refs"]
                .as_array()
                .expect("context refs")
                .len()
        })
        .collect::<Vec<_>>();
    assert_eq!(contexts, vec![1, 3]);
}

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
    assert!(events.iter().any(|event| event.event_type == "context"));
    assert!(events.iter().any(|event| event.event_type == "turn_end"));
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
    assert!(events.iter().any(|event| {
        event.event_type == "turn_end" && event.payload["status"] == json!("failed")
    }));
}
