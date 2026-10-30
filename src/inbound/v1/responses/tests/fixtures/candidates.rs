use super::*;

pub(crate) async fn spawn_failing_chat_upstream() -> String {
    async fn handler() -> (axum::http::StatusCode, Json<serde_json::Value>) {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "message": "upstream failed" } })),
        )
    }

    let app = Router::new().route("/chat/completions", post(handler));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let addr = listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    format!("http://{addr}")
}

pub(crate) async fn spawn_status_chat_upstream(status: axum::http::StatusCode) -> String {
    async fn handler(
        axum::extract::State(status): axum::extract::State<axum::http::StatusCode>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        (
            status,
            Json(json!({ "error": { "message": "upstream failed" } })),
        )
    }

    let app = Router::new()
        .route("/chat/completions", post(handler))
        .with_state(status);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let addr = listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    format!("http://{addr}")
}

pub(crate) async fn spawn_invalid_chat_upstream() -> String {
    async fn handler() -> Json<serde_json::Value> {
        Json(json!({ "model": "deepseek-chat" }))
    }

    let app = Router::new().route("/chat/completions", post(handler));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let addr = listener.local_addr().expect("upstream addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve upstream");
    });
    format!("http://{addr}")
}
