use super::*;

#[tokio::test]
async fn records_successful_anthropic_messages_activity() {
    let upstream = spawn_user_only_chat_upstream().await;
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
    let app = Router::new().nest(
        "/v1",
        super::router()
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(
                state.clone(),
                crate::routes::v1::auth::require_protocol_auth,
            )),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("read test server address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/messages"))
        .bearer_auth(&auth.token)
        .json(&json!({
            "model": "deepseek-chat",
            "max_tokens": 1024,
            "messages": [
                { "role": "user", "content": "hello" }
            ]
        }))
        .send()
        .await
        .expect("send request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let logs = state
        .storage
        .list_activities(&auth.access.0.identity_id, 10)
        .await
        .expect("load identity activity totals");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "success");
    assert_eq!(logs[0].input_tokens, Some(3));
    assert_eq!(logs[0].output_tokens, Some(4));

    server.abort();
}
