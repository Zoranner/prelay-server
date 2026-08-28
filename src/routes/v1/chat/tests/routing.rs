use super::*;

#[tokio::test]
async fn forwards_chat_completion_request_to_configured_upstream() {
    let upstream = spawn_chat_upstream().await;
    let state = test_state().await;
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

    let response = create_chat_completion(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        })),
    )
    .await
    .expect("create chat completion");

    let response = response_json(response).await;

    assert_eq!(response["id"], "chatcmpl_test");
    assert_eq!(
        response["choices"][0]["message"]["content"],
        "chat upstream hello"
    );
}

#[tokio::test]
async fn resolves_endpoint_model_name_to_upstream_model() {
    let upstream = spawn_alias_chat_upstream().await;
    let state = test_state().await;
    let provider = test_provider(
        "DeepSeek Provider",
        "openai_compatible",
        &upstream,
        "sk-upstream",
    )
    .await
    .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "coder", "deepseek-chat").await;

    let response = create_chat_completion(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "coder",
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        })),
    )
    .await
    .expect("create chat completion");

    let response = response_json(response).await;

    assert_eq!(response["model"], "deepseek-chat");
}

#[tokio::test]
async fn rejects_chat_when_provider_only_supports_responses_upstream_protocol() {
    let state = test_state().await;
    let provider = test_provider(
        "DeepSeek Provider",
        "openai",
        "http://127.0.0.1:1",
        "sk-upstream",
    )
    .await
    .expect("create provider");
    let auth = create_test_endpoint_auth(&state.storage, &provider, "coder", "deepseek-chat").await;

    let error = create_chat_completion(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "coder",
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        })),
    )
    .await
    .expect_err("unsupported provider protocol should fail before upstream");

    assert!(format!("{error:?}").contains("接入点未配置支持 chat_completions 的模型 coder"));
}

#[tokio::test]
async fn rejects_chat_when_provider_only_supports_anthropic_messages_upstream_protocol() {
    let state = test_state().await;
    let provider = test_provider(
        "Claude",
        "anthropic_compatible",
        "http://127.0.0.1:1",
        "sk-upstream",
    )
    .await
    .expect("create provider");
    let auth =
        create_test_endpoint_auth(&state.storage, &provider, "Claude", "claude-sonnet-4").await;

    let error = create_chat_completion(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "Claude",
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        })),
    )
    .await
    .expect_err("anthropic provider should not be exposed as chat completions");

    assert!(format!("{error:?}").contains("接入点未配置支持 chat_completions 的模型 Claude"));
}

#[tokio::test]
async fn forwards_chat_completion_to_protocol_specific_base_url() {
    let default_upstream = spawn_unexpected_chat_upstream().await;
    let chat_upstream = spawn_chat_upstream().await;
    let state = test_state().await;
    let provider = test_provider_with_capabilities(
        "deepseek-chat",
        "openai_compatible",
        &default_upstream,
        "sk-upstream",
        Some(&ProviderCapabilityOverrides {
            protocol_base_urls: Some(crate::models::ProviderProtocolBaseUrls {
                responses: None,
                openai: Some(chat_upstream),
                anthropic: None,
                ..Default::default()
            }),
            ..ProviderCapabilityOverrides::default()
        }),
    )
    .await
    .expect("create provider");
    let auth =
        create_test_endpoint_auth(&state.storage, &provider, "deepseek-chat", "deepseek-chat")
            .await;

    let response = create_chat_completion(
        State(state),
        auth.access,
        axum::Json(json!({
            "model": "deepseek-chat",
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        })),
    )
    .await
    .expect("create chat completion");
    let body = response_json(response).await;

    assert_eq!(
        body["choices"][0]["message"]["content"],
        "chat upstream hello"
    );
}
