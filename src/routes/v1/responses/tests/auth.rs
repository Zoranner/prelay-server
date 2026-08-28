use super::*;

#[tokio::test]
async fn rejects_unauthenticated_responses_request() {
    let state = crate::test_support::test_state().await;
    let app = Router::new().nest(
        "/v1",
        super::router()
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(
                state,
                crate::routes::v1::auth::require_protocol_auth,
            )),
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("route request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
