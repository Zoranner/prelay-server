use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

pub async fn request_status(
    app: &axum::Router,
    method: &str,
    path: &str,
    credential: Option<&str>,
) -> StatusCode {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(credential) = credential {
        builder = builder.header("authorization", format!("Bearer {credential}"));
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("build request"))
        .await
        .expect("route request")
        .status()
}
