use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde::de::DeserializeOwned;
use tower::ServiceExt;

pub async fn request_json<T: DeserializeOwned>(
    app: &axum::Router,
    method: &str,
    path: &str,
    credential: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, T) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(credential) = credential {
        builder = builder.header("authorization", format!("Bearer {credential}"));
    }
    let request = builder
        .header("content-type", "application/json")
        .body(Body::from(
            body.map(|value| value.to_string()).unwrap_or_default(),
        ))
        .expect("build request");
    let response = app.clone().oneshot(request).await.expect("route request");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response");
    (
        status,
        serde_json::from_slice(&bytes).expect("decode json response"),
    )
}
