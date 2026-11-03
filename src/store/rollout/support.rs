use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::activity::rollout::RolloutEvent;
use crate::domain::activity::{
    redaction::redact_json_value,
    rollout::{RolloutError, RolloutEventBody},
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

    pub(super) fn from_events(events: &[RolloutEvent]) -> Result<Self, RolloutError> {
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
        let canonical = serde_json::to_vec(&(event_type, comparison_payload(payload)))?;
        self.digest = chain_digest(self.digest, &canonical);
        self.count += 1;
        Ok(())
    }

    pub(super) fn matches_request_prefix(
        &self,
        request: &[RolloutEventBody],
    ) -> Result<Option<usize>, RolloutError> {
        if self.count == 0 {
            return Ok(Some(0));
        }

        let mut digest = [0; 32];
        let mut matched = 0usize;
        for body in request {
            let (event_type, payload) = event_parts(body)?;
            if !is_content_event(&event_type) {
                return Ok(None);
            }
            let canonical = serde_json::to_vec(&(event_type, comparison_payload(&payload)))?;
            digest = chain_digest(digest, &canonical);
            matched += 1;
            if matched as u64 == self.count {
                return Ok((digest == self.digest).then_some(matched));
            }
        }
        Ok(None)
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

pub(super) fn is_content_event(event_type: &str) -> bool {
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
