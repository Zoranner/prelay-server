use axum::http::HeaderMap;
use serde_json::Value;

use crate::activity::redact_sensitive_text;

const ERROR_MESSAGE_LIMIT: usize = 512;
const REQUEST_ID_HEADERS: &[&str] = &[
    "x-request-id",
    "x-requestid",
    "request-id",
    "x-amzn-requestid",
    "cf-ray",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpstreamObservability {
    pub request_id: Option<String>,
    pub error_message: Option<String>,
}

pub fn upstream_request_id(headers: &HeaderMap) -> Option<String> {
    REQUEST_ID_HEADERS.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub fn upstream_observability(headers: &HeaderMap, body: Option<&str>) -> UpstreamObservability {
    UpstreamObservability {
        request_id: upstream_request_id(headers),
        error_message: body.and_then(upstream_error_message),
    }
}

pub fn upstream_error_message(body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(body) {
        return error_message_from_json(&value)
            .or_else(|| plain_text_summary(body))
            .map(summarize_error_message);
    }

    plain_text_summary(body).map(summarize_error_message)
}

/// 上游失败时的统一文本：状态码加脱敏后的上游原因。
///
/// 响应体与请求记录共用同一份文本，避免记录里有原因、返回给客户端却没有。
pub fn upstream_failure_message(status: impl std::fmt::Display, body: &str) -> String {
    match upstream_error_message(body) {
        Some(message) => format!("上游请求失败: {status}: {message}"),
        None => format!("上游请求失败: {status}"),
    }
}

fn error_message_from_json(value: &Value) -> Option<String> {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.get("message").and_then(Value::as_str))
        .or_else(|| value.get("error").and_then(Value::as_str))
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(ToOwned::to_owned)
}

fn plain_text_summary(body: &str) -> Option<String> {
    let summary = body.split_whitespace().collect::<Vec<_>>().join(" ");
    (!summary.is_empty()).then_some(summary)
}

fn limit_summary(message: String) -> String {
    if message.len() <= ERROR_MESSAGE_LIMIT {
        return message;
    }

    let mut end = 0;
    for (index, _) in message.char_indices() {
        if index > ERROR_MESSAGE_LIMIT {
            break;
        }
        end = index;
    }
    format!("{}...", &message[..end])
}

fn summarize_error_message(message: String) -> String {
    limit_summary(redact_sensitive_text(&message))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{upstream_error_message, upstream_failure_message, upstream_request_id};

    #[test]
    fn extracts_request_id_from_common_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-ray", HeaderValue::from_static("cf-ray-123"));
        headers.insert("x-request-id", HeaderValue::from_static("req-123"));

        assert_eq!(upstream_request_id(&headers).as_deref(), Some("req-123"));
    }

    #[test]
    fn extracts_nested_error_message_from_json_body() {
        let body = r#"{"error":{"message":"provider overloaded","type":"rate_limit_error"}}"#;

        assert_eq!(
            upstream_error_message(body).as_deref(),
            Some("provider overloaded")
        );
    }

    #[test]
    fn truncates_plain_text_error_summary() {
        let body = "x".repeat(600);
        let message = upstream_error_message(&body).expect("error message");

        assert!(message.len() <= 515);
        assert!(message.ends_with("..."));
    }

    #[test]
    fn redacts_credentials_from_error_summary() {
        let body = r#"{"error":{"message":"rejected api_key=sk-live-secret-value"}}"#;
        let message = upstream_error_message(body).expect("error message");

        assert!(!message.contains("sk-live-secret-value"));
        assert!(message.contains("[REDACTED]"));
    }

    #[test]
    fn builds_status_prefixed_failure_message() {
        let body = r#"{"error":{"message":"upstream failed"}}"#;

        assert_eq!(
            upstream_failure_message("403 Forbidden", body),
            "上游请求失败: 403 Forbidden: upstream failed"
        );
        assert_eq!(
            upstream_failure_message("429 Too Many Requests", ""),
            "上游请求失败: 429 Too Many Requests"
        );
    }
}
