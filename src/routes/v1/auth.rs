use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};

use crate::{error::AppError, AppState};

#[derive(Clone, Debug)]
pub struct CurrentProtocolAccess {
    pub identity_id: String,
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub session_id: Option<String>,
}

pub async fn require_protocol_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let access = authenticate_protocol_request(&state, request.headers()).await?;
    let mut request = request;
    request.extensions_mut().insert(access);
    Ok(next.run(request).await)
}

pub async fn authenticate_protocol_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<CurrentProtocolAccess, AppError> {
    let token = extract_token(headers).ok_or(AppError::Unauthorized)?;
    state
        .storage
        .authenticate_protocol_access(&token)
        .await?
        .map(|access| CurrentProtocolAccess {
            identity_id: access.identity_id,
            endpoint_id: access.endpoint_id,
            endpoint_name: access.endpoint_name,
            session_id: extract_session_id(headers),
        })
        .ok_or(AppError::Unauthorized)
}

pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_string)
        })
}

const SESSION_ID_HEADERS: [&str; 2] = ["session_id", "x-session-id"];
const MAX_SESSION_ID_LENGTH: usize = 128;

fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    SESSION_ID_HEADERS.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= MAX_SESSION_ID_LENGTH)
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::extract_session_id;

    #[test]
    fn extracts_supported_session_headers_in_priority_order() {
        let mut headers = HeaderMap::new();
        headers.insert("session_id", HeaderValue::from_static("ses_codex"));
        headers.insert("x-session-id", HeaderValue::from_static("ses_generic"));
        assert_eq!(extract_session_id(&headers), Some("ses_codex".to_string()));
    }

    #[test]
    fn ignores_blank_and_oversized_session_ids() {
        let mut headers = HeaderMap::new();
        headers.insert("session_id", HeaderValue::from_static("  "));
        assert_eq!(extract_session_id(&headers), None);

        headers.insert(
            "session_id",
            HeaderValue::from_bytes("a".repeat(129).as_bytes()).expect("header value"),
        );
        assert_eq!(extract_session_id(&headers), None);
    }
}
