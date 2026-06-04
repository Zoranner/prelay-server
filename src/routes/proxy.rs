use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
};
use futures::TryStreamExt;

use crate::{db, error::AppError, AppState};

/// Headers that must not be forwarded (hop-by-hop)
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

pub async fn handle(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, AppError> {
    // Extract internal token from Authorization or x-api-key header
    let token = extract_token(&req).ok_or(AppError::Unauthorized)?;

    // Look up provider config by token
    let config = db::get_config_by_token(&state.db, &token)
        .await?
        .ok_or(AppError::Unauthorized)?;

    // Construct upstream URL: {base_url}{request_path}[?query]
    let path = req.uri().path();
    let query_str = req
        .uri()
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let upstream_url = format!(
        "{}{}{}",
        config.base_url.trim_end_matches('/'),
        path,
        query_str
    );

    tracing::info!(
        method = %req.method(),
        path = %path,
        upstream = %upstream_url,
        provider = %config.name,
        "Proxying request"
    );

    // Build upstream reqwest request
    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let mut upstream_req = state.client.request(method, &upstream_url);

    // Forward safe headers, skip auth and hop-by-hop headers
    for (name, value) in req.headers() {
        let n = name.as_str().to_lowercase();
        if HOP_BY_HOP.contains(&n.as_str())
            || n == "authorization"
            || n == "x-api-key"
            || n == "host"
        {
            continue;
        }
        upstream_req = upstream_req.header(name.as_str(), value);
    }

    // Set authentication header based on provider type
    if config.uses_anthropic_auth() {
        upstream_req = upstream_req
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        upstream_req = upstream_req.header("authorization", format!("Bearer {}", config.api_key));
    }

    // Forward request body (up to 32 MB)
    let body_bytes = axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    if !body_bytes.is_empty() {
        upstream_req = upstream_req.body(body_bytes);
    }

    // Send the request to upstream provider
    let upstream_resp = upstream_req
        .send()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    // Map upstream status code
    let status = StatusCode::from_u16(upstream_resp.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    // Build response, forwarding upstream headers
    let mut builder = Response::builder().status(status);
    for (name, value) in upstream_resp.headers() {
        if !HOP_BY_HOP.contains(&name.as_str()) {
            builder = builder.header(name.as_str(), value);
        }
    }

    // Stream response body back to client (supports SSE streaming)
    let stream = upstream_resp.bytes_stream().map_err(std::io::Error::other);
    let body = Body::from_stream(stream);

    builder.body(body).map_err(|e| AppError::Internal(e.into()))
}

fn extract_token(req: &Request<Body>) -> Option<String> {
    // Try Authorization: Bearer <token>
    if let Some(auth) = req.headers().get("authorization") {
        if let Ok(s) = auth.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ") {
                let t = token.trim().to_string();
                if !t.is_empty() {
                    return Some(t);
                }
            }
        }
    }
    // Try x-api-key: <token>
    if let Some(key) = req.headers().get("x-api-key") {
        if let Ok(s) = key.to_str() {
            let t = s.trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}
