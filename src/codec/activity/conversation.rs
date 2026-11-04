use serde_json::{json, Map, Value};

use crate::domain::activity::{
    redact_sensitive_text, redaction::redact_json_value, ContentDraft, ContentEventBody,
    ConversationMessage, ConversationReasoning, ConversationToolCall, ConversationToolResult,
    ConversationUsage, EventPhase,
};

const OMITTED_MEDIA_TYPES: [&str; 11] = [
    "audio",
    "file",
    "image",
    "image_url",
    "input_audio",
    "input_file",
    "input_image",
    "output_audio",
    "output_file",
    "output_image",
    "video",
];

const OMITTED_MEDIA_KEYS: [&str; 10] = [
    "audio_data",
    "b64_json",
    "base64",
    "file_data",
    "file_id",
    "filename",
    "image_url",
    "input_audio",
    "input_image",
    "sha256",
];

pub fn responses_request(payload: &Value, input_items: &[Value]) -> Vec<ContentDraft> {
    let mut events = Vec::new();
    if let Some(instructions) = payload
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        events.push(ContentDraft::new(ContentEventBody::Message(
            ConversationMessage {
                phase: EventPhase::Request,
                role: "system".to_string(),
                message_phase: None,
                content: Value::String(redact_sensitive_text(instructions)),
            },
        )));
    }
    for item in input_items {
        events.extend(response_item(item, EventPhase::Request));
    }
    events
}

pub(super) fn response_item(item: &Value, phase: EventPhase) -> Vec<ContentDraft> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
    match item_type {
        "message" | "" => message_event(item, phase).into_iter().collect(),
        "reasoning" => reasoning_event(item, phase).into_iter().collect(),
        "configuration_update" | "item_reference" => Vec::new(),
        item_type if is_tool_result_type(item_type) => {
            tool_result_event(item, phase).into_iter().collect()
        }
        item_type if is_tool_call_type(item_type) => {
            tool_call_event(item, phase).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

fn message_event(item: &Value, phase: EventPhase) -> Option<ContentDraft> {
    let role = item.get("role").and_then(Value::as_str)?.trim();
    let message_phase = (role == "assistant")
        .then(|| {
            item.get("message_phase")
                .or_else(|| item.get("phase"))
                .and_then(Value::as_str)
                .filter(|value| matches!(*value, "commentary" | "final_answer"))
                .map(str::to_string)
        })
        .flatten();
    let content = message_content(item.get("content")?)?;
    let draft = ContentDraft::new(ContentEventBody::Message(ConversationMessage {
        phase,
        role: role.to_string(),
        message_phase,
        content,
    }));
    Some(with_item_id(draft, item))
}

fn reasoning_event(item: &Value, phase: EventPhase) -> Option<ContentDraft> {
    let summary = readable_text(item.get("summary"));
    let content = readable_text(item.get("content"));
    if summary.is_none() && content.is_none() {
        return None;
    }
    let draft = ContentDraft::new(ContentEventBody::Reasoning(ConversationReasoning {
        phase,
        summary,
        content,
    }));
    Some(with_item_id(draft, item))
}

fn tool_call_event(item: &Value, phase: EventPhase) -> Option<ContentDraft> {
    let item_type = item.get("type").and_then(Value::as_str)?;
    let tool_call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(item_type)
        .to_string();
    let arguments = item
        .get("arguments")
        .or_else(|| item.get("input"))
        .map(sanitize_json)
        .unwrap_or_else(|| tool_arguments_remainder(item));
    let draft = ContentDraft::new(ContentEventBody::ToolCall(ConversationToolCall {
        phase,
        tool_call_id,
        name,
        arguments,
    }));
    Some(with_item_id(draft, item))
}

fn tool_result_event(item: &Value, phase: EventPhase) -> Option<ContentDraft> {
    let tool_call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let content = item
        .get("output")
        .or_else(|| item.get("result"))
        .or_else(|| item.get("content"))
        .map(sanitize_json)
        .unwrap_or(Value::String(String::new()));
    let draft = ContentDraft::new(ContentEventBody::ToolResult(ConversationToolResult {
        phase,
        tool_call_id,
        content,
    }));
    Some(with_item_id(draft, item))
}

fn is_tool_call_type(item_type: &str) -> bool {
    item_type == "function_call" || item_type == "custom_tool_call" || item_type.ends_with("_call")
}

fn is_tool_result_type(item_type: &str) -> bool {
    item_type == "function_call_output"
        || item_type == "custom_tool_call_output"
        || item_type.ends_with("_call_output")
}

pub(super) fn with_item_id(draft: ContentDraft, item: &Value) -> ContentDraft {
    item.get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map_or(draft.clone(), |id| draft.with_item_id(id))
}

fn tool_arguments_remainder(item: &Value) -> Value {
    let Some(object) = item.as_object() else {
        return Value::Object(Map::new());
    };
    let mut arguments = object.clone();
    for key in [
        "type",
        "id",
        "call_id",
        "name",
        "status",
        "output",
        "result",
        "encrypted_content",
    ] {
        arguments.remove(key);
    }
    sanitize_json(&Value::Object(arguments))
}

pub(super) fn message_content(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) if !text.is_empty() => Some(Value::String(redact_sensitive_text(text))),
        Value::String(_) | Value::Null => None,
        Value::Array(parts) => {
            let parts = parts.iter().filter_map(content_part).collect::<Vec<_>>();
            if parts.is_empty() {
                return None;
            }
            if parts.len() == 1 {
                if let Some(text) = parts[0].get("text").and_then(Value::as_str) {
                    if parts[0].get("type").and_then(Value::as_str) != Some("refusal") {
                        return Some(Value::String(text.to_string()));
                    }
                }
            }
            Some(Value::Array(parts))
        }
        _ => None,
    }
}

fn content_part(part: &Value) -> Option<Value> {
    let part_type = part.get("type").and_then(Value::as_str).unwrap_or("text");
    if is_media_type(part_type) {
        return Some(omitted_media(part_type));
    }
    match part_type {
        "refusal" => part
            .get("refusal")
            .and_then(Value::as_str)
            .map(|text| json!({ "type": "refusal", "text": redact_sensitive_text(text) })),
        "input_text" | "output_text" | "text" => part
            .get("text")
            .and_then(Value::as_str)
            .map(|text| json!({ "type": part_type, "text": redact_sensitive_text(text) })),
        _ => None,
    }
}

fn readable_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| redact_sensitive_text(text))
        }
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            let text = text.trim();
            (!text.is_empty()).then(|| redact_sensitive_text(text))
        }
        _ => None,
    }
}

pub(super) fn sanitize_json(value: &Value) -> Value {
    match value {
        Value::String(text) if looks_like_binary(text) => omitted_media("binary"),
        Value::String(text) => {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                if parsed.is_object() || parsed.is_array() {
                    return sanitize_json(&parsed);
                }
            }
            Value::String(redact_sensitive_text(text))
        }
        Value::Array(items) => Value::Array(items.iter().map(sanitize_json).collect()),
        Value::Object(object) => {
            if let Some(part_type) = object.get("type").and_then(Value::as_str) {
                if is_media_type(part_type) {
                    return omitted_media(part_type);
                }
            }
            let sanitized = object
                .iter()
                .filter(|(key, _)| !is_omitted_key(key))
                .map(|(key, value)| (key.clone(), sanitize_json(value)))
                .collect::<Map<_, _>>();
            redact_json_value(&Value::Object(sanitized))
        }
        _ => value.clone(),
    }
}

fn is_media_type(value: &str) -> bool {
    OMITTED_MEDIA_TYPES.contains(&value)
}

fn is_omitted_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    OMITTED_MEDIA_KEYS.contains(&key.as_str())
        || matches!(
            key.as_str(),
            "hash" | "size_bytes" | "file_name" | "authorization" | "cookie" | "api_key"
        )
}

pub(super) fn omitted_media(part_type: &str) -> Value {
    json!({ "type": part_type, "omitted": true })
}

fn looks_like_binary(value: &str) -> bool {
    value.starts_with("data:") && value.contains(";base64,")
}

pub(super) fn usage_event(usage: Option<&Value>) -> Option<ContentDraft> {
    let usage = usage?;
    let input_tokens = number(usage, &["input_tokens", "prompt_tokens"]);
    let output_tokens = number(usage, &["output_tokens", "completion_tokens"]);
    let reasoning_tokens = number_at(
        usage,
        &[
            "/output_tokens_details/reasoning_tokens",
            "/completion_tokens_details/reasoning_tokens",
            "/reasoning_tokens",
        ],
    );
    let cache_read_tokens = number_at(
        usage,
        &[
            "/input_tokens_details/cached_tokens",
            "/prompt_tokens_details/cached_tokens",
            "/cache_read_input_tokens",
        ],
    );
    let cache_write_tokens = number_at(
        usage,
        &[
            "/input_tokens_details/cache_write_tokens",
            "/prompt_tokens_details/cache_write_tokens",
            "/cache_creation_input_tokens",
        ],
    );
    if input_tokens.is_none()
        && output_tokens.is_none()
        && reasoning_tokens.is_none()
        && cache_read_tokens.is_none()
        && cache_write_tokens.is_none()
    {
        return None;
    }
    Some(ContentDraft::new(ContentEventBody::Usage(
        ConversationUsage {
            input_tokens,
            output_tokens,
            reasoning_tokens,
            cache_read_tokens,
            cache_write_tokens,
        },
    )))
}

fn number(usage: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| usage.get(*key).and_then(Value::as_i64))
}

fn number_at(usage: &Value, pointers: &[&str]) -> Option<i64> {
    pointers
        .iter()
        .find_map(|pointer| usage.pointer(pointer).and_then(Value::as_i64))
}
