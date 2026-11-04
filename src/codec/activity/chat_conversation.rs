use serde_json::{json, Value};

use crate::domain::activity::{
    redact_sensitive_text, ContentDraft, ContentEventBody, ConversationMessage,
    ConversationReasoning, ConversationToolCall, ConversationToolResult, EventPhase,
};

use super::conversation::{message_content, omitted_media, sanitize_json, usage_event};

pub fn chat_request(payload: &Value) -> Vec<ContentDraft> {
    payload
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|message| chat_message(message, EventPhase::Request))
        .collect()
}

pub fn chat_response(payload: &Value) -> Vec<ContentDraft> {
    let mut events = payload
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|choice| {
            choice
                .get("message")
                .map(|message| chat_message(message, EventPhase::Response))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    if let Some(usage) = usage_event(payload.get("usage")) {
        events.push(usage);
    }
    events
}

pub fn image_request(payload: &Value) -> Vec<ContentDraft> {
    payload
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(|prompt| {
            vec![ContentDraft::new(ContentEventBody::Message(
                ConversationMessage {
                    phase: EventPhase::Request,
                    role: "user".to_string(),
                    message_phase: None,
                    content: Value::String(redact_sensitive_text(prompt)),
                },
            ))]
        })
        .unwrap_or_default()
}

pub fn image_response() -> Vec<ContentDraft> {
    vec![ContentDraft::new(ContentEventBody::Message(
        ConversationMessage {
            phase: EventPhase::Response,
            role: "assistant".to_string(),
            message_phase: None,
            content: Value::Array(vec![omitted_media("output_image")]),
        },
    ))]
}

fn chat_message(message: &Value, phase: EventPhase) -> Vec<ContentDraft> {
    let mut events = Vec::new();
    let mut content = message.get("content").and_then(message_content);
    if let Some(refusal) = message.get("refusal").and_then(Value::as_str) {
        let refusal = json!({ "type": "refusal", "text": redact_sensitive_text(refusal) });
        content = Some(match content {
            Some(Value::Array(mut parts)) => {
                parts.push(refusal);
                Value::Array(parts)
            }
            Some(Value::String(text)) => {
                Value::Array(vec![json!({ "type": "text", "text": text }), refusal])
            }
            Some(other) => other,
            None => Value::Array(vec![refusal]),
        });
    }
    let tool_call_id = message.get("tool_call_id").and_then(Value::as_str);
    if let Some(content) = content {
        if let Some(tool_call_id) = tool_call_id {
            events.push(ContentDraft::new(ContentEventBody::ToolResult(
                ConversationToolResult {
                    phase: phase.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    content,
                },
            )));
        } else {
            events.push(ContentDraft::new(ContentEventBody::Message(
                ConversationMessage {
                    phase: phase.clone(),
                    role: message
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("user")
                        .to_string(),
                    message_phase: None,
                    content,
                },
            )));
        }
    }
    if let Some(reasoning) = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        events.push(ContentDraft::new(ContentEventBody::Reasoning(
            ConversationReasoning {
                phase: phase.clone(),
                summary: None,
                content: Some(redact_sensitive_text(reasoning)),
            },
        )));
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let tool_call_id = tool_call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let name = tool_call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = tool_call
                .pointer("/function/arguments")
                .map(sanitize_json)
                .unwrap_or_else(|| Value::Object(Default::default()));
            events.push(ContentDraft::new(ContentEventBody::ToolCall(
                ConversationToolCall {
                    phase: phase.clone(),
                    tool_call_id: tool_call_id.to_string(),
                    name: name.to_string(),
                    arguments,
                },
            )));
        }
    }
    events
}
