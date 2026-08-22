use axum::{extract::DefaultBodyLimit, middleware, Router};

use crate::AppState;

const MAX_PROTOCOL_REQUEST_BODY_BYTES: usize = 32 * 1024 * 1024;

pub mod auth;
mod chat;
mod endpoint_resolver;
mod messages;
mod models;
mod responses;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(chat::router())
        .merge(messages::router())
        .merge(models::router())
        .merge(responses::router())
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(MAX_PROTOCOL_REQUEST_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            state,
            auth::require_protocol_auth,
        ))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        extract::{DefaultBodyLimit, Json},
        http::{Request, StatusCode},
        routing::post,
        Router,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::MAX_PROTOCOL_REQUEST_BODY_BYTES;

    #[tokio::test]
    async fn protocol_body_limit_accepts_image_sized_json_requests() {
        let payload = format!("{{\"input\":\"{}\"}}", "a".repeat(3 * 1024 * 1024));
        let app = Router::new()
            .route(
                "/request",
                post(|Json(_): Json<Value>| async { StatusCode::NO_CONTENT }),
            )
            .layer(DefaultBodyLimit::max(MAX_PROTOCOL_REQUEST_BODY_BYTES));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/request")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .expect("build request"),
            )
            .await
            .expect("route request");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
