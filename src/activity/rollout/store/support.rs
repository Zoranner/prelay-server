use serde_json::Value;

use super::RolloutEvent;
use crate::activity::{
    redaction::redact_json_value,
    rollout::{RolloutError, RolloutEventBody},
};

pub(super) fn is_safe_path_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(super) fn last_window_generation(events: &[RolloutEvent]) -> Option<u64> {
    events.iter().rev().find_map(|event| {
        (event.event_type == "window_start")
            .then(|| event.payload.get("generation").and_then(Value::as_u64))
            .flatten()
    })
}

pub(super) fn common_prefix_len(
    existing: &[RolloutEvent],
    request: &[RolloutEventBody],
) -> Result<usize, RolloutError> {
    let mut existing_content = existing
        .iter()
        .filter(|event| is_content_event(&event.event_type));
    let mut matched = 0;
    for body in request {
        let Some(event) = existing_content.next() else {
            break;
        };
        let (event_type, payload) = event_parts(body)?;
        if event.event_type == event_type
            && comparison_payload(&event.payload) == comparison_payload(&payload)
        {
            matched += 1;
        } else {
            break;
        }
    }
    Ok(matched)
}

fn is_content_event(event_type: &str) -> bool {
    !matches!(
        event_type,
        "session_meta" | "window_start" | "media" | "usage" | "error" | "session_end"
    )
}

fn comparison_payload(payload: &Value) -> Value {
    let mut payload = payload.clone();
    if let Some(object) = payload.as_object_mut() {
        object.remove("phase");
    }
    payload
}

pub(super) fn event_parts(body: &RolloutEventBody) -> Result<(String, Value), RolloutError> {
    let body_value = serde_json::to_value(body)?;
    let object = body_value
        .as_object()
        .ok_or_else(|| RolloutError::InvalidPath("event body is not an object".to_string()))?;
    let event_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| RolloutError::InvalidPath("event type is missing".to_string()))?
        .to_string();
    let payload = redact_json_value(&object.get("payload").cloned().unwrap_or(Value::Null));
    Ok((event_type, payload))
}
