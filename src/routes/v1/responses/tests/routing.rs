use super::*;

#[tokio::test]
async fn forwards_responses_request_to_chat_completions_upstream() {
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

    let response = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect("create response");

    let response = response_json(response).await;

    assert_eq!(response["object"], "response");
    assert_eq!(response["model"], "deepseek-chat");
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        "upstream hello"
    );
    assert_eq!(response["usage"]["input_tokens"], 3);
    assert_eq!(response["usage"]["output_tokens"], 4);
}

#[tokio::test]
async fn forwards_responses_request_to_chat_bridge_before_anthropic_for_multi_protocol_provider() {
    let upstream = spawn_chat_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider(
        "deepseek-chat",
        "kimi_coding_anthropic",
        &upstream,
        "sk-upstream",
    )
    .await
    .expect("create provider");
    let auth =
        create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
            .await;

    let response = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "input": "hello"
        })),
    )
    .await
    .expect("create response");
    let response = response_json(response).await;

    assert_eq!(response["object"], "response");
    assert_eq!(response["model"], "deepseek-chat");
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        "upstream hello"
    );
}

#[tokio::test]
async fn forwards_responses_request_to_native_upstream() {
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
            "input": "hello"
        })),
    )
    .await
    .expect("create response");
    let response = response_json(response).await;

    assert_eq!(response["id"], "resp_native");
    assert_eq!(response["object"], "response");
    assert_eq!(response["model"], "gpt-4.1");
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        "native response"
    );
}

#[tokio::test]
async fn forwards_non_streaming_responses_request_to_anthropic_messages_upstream() {
    let upstream = spawn_native_anthropic_messages_upstream().await;
    let state = crate::test_support::test_state().await;
    let provider = test_provider(
        "claude-sonnet",
        "anthropic_compatible",
        &upstream,
        "sk-upstream",
    )
    .await
    .expect("create provider");
    let auth =
        create_test_endpoint_auth(&state.storage, &provider, "claude-sonnet", "claude-sonnet")
            .await;

    let response = create_response(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "claude-sonnet",
            "input": "hello"
        })),
    )
    .await
    .expect("create response");
    let response = response_json(response).await;

    assert_eq!(response["object"], "response");
    assert_eq!(response["model"], "claude-sonnet");
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        "anthropic hello"
    );
    assert_eq!(response["usage"]["input_tokens"], 3);
    assert_eq!(response["usage"]["output_tokens"], 4);
}
