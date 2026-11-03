use super::*;

pub(crate) async fn spawn_rewriting_native_responses_upstream() -> String {
    pub(super) async fn handler(
        Json(payload): Json<serde_json::Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;

        assert_eq!(payload["model"], "gpt-4.1");
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["store"], false);
        assert_eq!(
            payload["input"],
            json!([{ "role": "user", "content": "hello" }])
        );
        (
            [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
            concat!(
                "event: response.created\n",
                "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_upstream\",\"status\":\"in_progress\"}}\n\n",
                "event: response.output_item.added\n",
                "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"item_upstream\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"read_file\",\"arguments\":\"\"}}\n\n",
                "event: response.function_call_arguments.delta\n",
                "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_upstream\",\"call_id\":\"call_1\",\"delta\":\"{}\"}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\",\"response_id\":\"resp_upstream\",\"response\":{\"id\":\"resp_upstream\",\"status\":\"completed\",\"output\":[{\"type\":\"reasoning\",\"id\":\"rs_1\"},{\"type\":\"function_call\",\"id\":\"item_upstream\",\"call_id\":\"call_1\",\"name\":\"read_file\",\"arguments\":\"{}\"}],\"usage\":{\"input_tokens\":3,\"output_tokens\":4}}}\n\n",
                "data: [DONE]\n\n",
            ),
        )
            .into_response()
    }

    let app = Router::new().route("/responses", post(handler));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let addr = listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    format!("http://{addr}")
}
