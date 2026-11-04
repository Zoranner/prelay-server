use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::activity::{
    redaction::redact_json_value,
    rollout::{
        ContentDraft, ContentEventBody, ConversationEvent, ConversationEventBody, RolloutError,
    },
};

#[derive(Debug, Clone)]
pub(super) struct ContentHash {
    pub(super) count: u64,
    digest: [u8; 32],
}

impl Default for ContentHash {
    fn default() -> Self {
        Self::from_parts(0, [0; 32])
    }
}

impl ContentHash {
    pub(super) fn from_parts(count: u64, digest: [u8; 32]) -> Self {
        Self { count, digest }
    }

    pub(super) fn digest_bytes(&self) -> [u8; 32] {
        self.digest
    }

    pub(super) fn from_events(events: &[ConversationEvent]) -> Result<Self, RolloutError> {
        let mut chain = Self::default();
        for event in events
            .iter()
            .filter(|event| is_content_event(&event.event_type))
        {
            chain.append(&event.event_type, &event.payload)?;
        }
        Ok(chain)
    }

    pub(super) fn append(&mut self, event_type: &str, payload: &Value) -> Result<(), RolloutError> {
        let canonical = canonical_event(event_type, payload)?;
        self.digest = chain_digest(self.digest, &canonical);
        self.count += 1;
        Ok(())
    }

    #[cfg(test)]
    fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

fn chain_digest(previous: [u8; 32], canonical: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(previous);
    hasher.update((canonical.len() as u64).to_le_bytes());
    hasher.update(canonical);
    hasher.finalize().into()
}

fn canonical_event(event_type: &str, payload: &Value) -> Result<Vec<u8>, RolloutError> {
    Ok(serde_json::to_vec(&(
        event_type,
        comparison_payload(payload),
    ))?)
}

pub(super) fn event_identity(event_type: &str, payload: &Value) -> Result<[u8; 32], RolloutError> {
    Ok(Sha256::digest(canonical_event(event_type, payload)?).into())
}

pub(super) fn is_safe_path_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(super) fn is_content_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "message" | "reasoning" | "tool_call" | "tool_result"
    )
}

fn comparison_payload(payload: &Value) -> Value {
    let mut payload = payload.clone();
    if let Some(object) = payload.as_object_mut() {
        object.remove("phase");
    }
    payload
}

pub(super) fn content_event_parts(
    body: &ContentEventBody,
) -> Result<(String, Value), RolloutError> {
    body_parts(&serde_json::to_value(body)?)
}

pub(super) fn event_parts(body: &ConversationEventBody) -> Result<(String, Value), RolloutError> {
    body_parts(&serde_json::to_value(body)?)
}

fn body_parts(body_value: &Value) -> Result<(String, Value), RolloutError> {
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

pub(super) fn item_id(body: &ContentEventBody) -> Option<String> {
    match body {
        ContentEventBody::ToolCall(tool_call) => Some(tool_call.tool_call_id.clone()),
        ContentEventBody::ToolResult(tool_result) => Some(tool_result.tool_call_id.clone()),
        _ => None,
    }
}

pub(super) fn item_ref_key(event_type: &str, id: &str) -> String {
    format!("{event_type}:{id}")
}

pub(super) fn draft_event_parts(draft: &ContentDraft) -> Result<(String, Value), RolloutError> {
    content_event_parts(&draft.body)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ContentHash;

    #[test]
    fn content_chain_ignores_object_key_order_and_phase() {
        let mut request = ContentHash::default();
        request
            .append(
                "message",
                &json!({
                    "phase": "request",
                    "role": "user",
                    "content": { "b": 2, "a": 1 }
                }),
            )
            .expect("hash request event");

        let mut response = ContentHash::default();
        response
            .append(
                "message",
                &json!({
                    "content": { "a": 1, "b": 2 },
                    "role": "user",
                    "phase": "response"
                }),
            )
            .expect("hash response event");

        assert_eq!(request.digest(), response.digest());
    }
}
