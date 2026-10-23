use axum::{extract::DefaultBodyLimit, middleware, Router};

use crate::{protocol_body_limit, AppState};

pub mod auth;
mod candidates;
mod chat;
mod endpoint_resolver;
mod images;
mod models;
mod responses;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(chat::router())
        .merge(images::router())
        .merge(models::router())
        .merge(responses::router())
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(protocol_body_limit::limit()))
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

    use crate::protocol_body_limit;

    #[tokio::test]
    async fn protocol_body_limit_accepts_screenshot_session_payloads() {
        let payload = format!("{{\"input\":\"{}\"}}", "a".repeat(33 * 1024 * 1024));
        let app = Router::new()
            .route(
                "/request",
                post(|Json(_): Json<Value>| async { StatusCode::NO_CONTENT }),
            )
            .layer(DefaultBodyLimit::max(protocol_body_limit::limit()));

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
