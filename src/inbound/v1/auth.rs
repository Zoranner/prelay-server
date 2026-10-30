use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};

use crate::{domain::activity::rollout::SessionContext, error::AppError, AppState};

#[derive(Clone, Debug)]
pub struct CurrentProtocolAccess {
    pub identity_id: String,
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub window_id: Option<String>,
    pub window_generation: Option<u64>,
}

impl CurrentProtocolAccess {
    pub fn session_context(&self, fallback_thread_id: &str) -> SessionContext {
        let fallback_thread_id = if is_safe_session_segment(fallback_thread_id) {
            fallback_thread_id.to_string()
        } else {
            format!("conv_{}", uuid::Uuid::new_v4().simple())
        };
        let thread_id = self
            .thread_id
            .as_deref()
            .or(self.session_id.as_deref())
            .filter(|value| is_safe_session_segment(value))
            .map(str::to_string)
            .unwrap_or(fallback_thread_id);
        SessionContext::with_window(
            thread_id,
            self.window_id.clone(),
            self.window_generation.unwrap_or(0),
        )
    }
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
        .map(|access| {
            let window = extract_window(headers);
            CurrentProtocolAccess {
                identity_id: access.identity_id,
                endpoint_id: access.endpoint_id,
                endpoint_name: access.endpoint_name,
                session_id: extract_session_id(headers),
                thread_id: extract_thread_id(headers),
                window_id: window.as_ref().map(|window| window.0.clone()),
                window_generation: window.map(|window| window.1),
            }
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

const SESSION_ID_HEADERS: [&str; 3] = ["session-id", "x-session-id", "session_id"];
const THREAD_ID_HEADERS: [&str; 4] = ["thread-id", "session-id", "x-session-id", "session_id"];
const MAX_SESSION_ID_LENGTH: usize = 128;
const MAX_WINDOW_ID_LENGTH: usize = 256;

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

fn extract_thread_id(headers: &HeaderMap) -> Option<String> {
    THREAD_ID_HEADERS.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| is_safe_session_segment(value))
            .map(str::to_string)
    })
}

fn extract_window(headers: &HeaderMap) -> Option<(String, u64)> {
    let value = headers
        .get("x-codex-window-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= MAX_WINDOW_ID_LENGTH)?;
    let (thread_id, generation) = value.rsplit_once(':')?;
    if thread_id.is_empty() || !is_safe_session_segment(thread_id) {
        return None;
    }
    let generation = generation.parse::<u64>().ok()?;
    Some((value.to_string(), generation))
}

fn is_safe_session_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SESSION_ID_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{extract_session_id, extract_thread_id, extract_window};

    #[test]
    fn extracts_supported_session_headers_in_priority_order() {
        let mut headers = HeaderMap::new();
        headers.insert("session_id", HeaderValue::from_static("ses_codex"));
        headers.insert("x-session-id", HeaderValue::from_static("ses_generic"));
        assert_eq!(
            extract_session_id(&headers),
            Some("ses_generic".to_string())
        );

        headers.insert("session-id", HeaderValue::from_static("ses_header"));
        assert_eq!(extract_session_id(&headers), Some("ses_header".to_string()));
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

    #[test]
    fn prefers_codex_thread_and_session_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("session_id", HeaderValue::from_static("legacy_session"));
        headers.insert("x-session-id", HeaderValue::from_static("generic_session"));
        headers.insert("session-id", HeaderValue::from_static("codex_session"));
        headers.insert("thread-id", HeaderValue::from_static("codex_thread"));

        assert_eq!(
            extract_session_id(&headers),
            Some("codex_session".to_string())
        );
        assert_eq!(
            extract_thread_id(&headers),
            Some("codex_thread".to_string())
        );
    }

    #[test]
    fn rejects_path_characters_and_oversized_thread_ids() {
        let mut headers = HeaderMap::new();
        headers.insert("thread-id", HeaderValue::from_static("../thread"));
        assert_eq!(extract_thread_id(&headers), None);

        headers.insert(
            "thread-id",
            HeaderValue::from_bytes("a".repeat(129).as_bytes()).expect("header value"),
        );
        assert_eq!(extract_thread_id(&headers), None);
    }

    #[test]
    fn parses_the_codex_window_generation() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-codex-window-id",
            HeaderValue::from_static("thread-123:7"),
        );

        assert_eq!(
            extract_window(&headers),
            Some(("thread-123:7".to_string(), 7))
        );
    }
}
