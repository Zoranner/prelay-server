use serde_json::{json, Value};

use crate::{
    codec::internal::{InternalContentPart, InternalOutputItem, InternalResponse, InternalRole},
    domain::activity::{
        redact_sensitive_text, ContentDraft, ContentEventBody, ConversationMessage,
        ConversationReasoning, ConversationToolCall, ConversationUsage, EventPhase,
    },
};

use super::conversation::{omitted_media, response_item, usage_event};

pub fn responses_output(output: &Value, usage: Option<&Value>) -> Vec<ContentDraft> {
    let mut events = output
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| response_item(item, EventPhase::Response))
        .collect::<Vec<_>>();
    if let Some(usage) = usage_event(usage) {
        events.push(usage);
    }
    events
}

pub fn internal_response(response: &InternalResponse) -> Vec<ContentDraft> {
    let mut events = Vec::new();
    for output in &response.output {
        match output {
            InternalOutputItem::Message { id, role, content } => {
                if let Some(content) = internal_message_content(content, role) {
                    let draft = ContentDraft::new(ContentEventBody::Message(ConversationMessage {
                        phase: EventPhase::Response,
                        role: role_name(role).to_string(),
                        message_phase: None,
                        content,
                    }));
                    events.push(draft.with_item_id(id));
                }
            }
            InternalOutputItem::FunctionToolCall {
                id,
                name,
                arguments,
                reasoning_content,
            } => {
                if let Some(reasoning) = reasoning_content
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                {
                    events.push(ContentDraft::new(ContentEventBody::Reasoning(
                        ConversationReasoning {
                            phase: EventPhase::Response,
                            summary: None,
                            content: Some(redact_sensitive_text(reasoning)),
                        },
                    )));
                }
                events.push(ContentDraft::new(ContentEventBody::ToolCall(
                    ConversationToolCall {
                        phase: EventPhase::Response,
                        tool_call_id: id.clone(),
                        name: name.clone(),
                        arguments: Value::String(arguments.clone()),
                    },
                )));
            }
        }
    }
    if let Some(usage) = &response.usage {
        events.push(ContentDraft::new(ContentEventBody::Usage(
            ConversationUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                reasoning_tokens: usage.reasoning_tokens,
                cache_read_tokens: usage.cache_read_tokens,
                cache_write_tokens: usage.cache_write_tokens,
            },
        )));
    }
    events
}

fn internal_message_content(content: &[InternalContentPart], role: &InternalRole) -> Option<Value> {
    let parts = content
        .iter()
        .filter_map(|part| match part {
            InternalContentPart::Text(text) if !text.is_empty() => {
                let part_type = if matches!(role, InternalRole::Assistant) {
                    "output_text"
                } else {
                    "input_text"
                };
                Some(json!({ "type": part_type, "text": redact_sensitive_text(text) }))
            }
            InternalContentPart::Text(_) => None,
            InternalContentPart::Image(_) => Some(omitted_media("input_image")),
        })
        .collect::<Vec<_>>();
    match parts.len() {
        0 => None,
        1 if parts[0].get("text").is_some() => parts[0].get("text").cloned(),
        _ => Some(Value::Array(parts)),
    }
}

fn role_name(role: &InternalRole) -> &'static str {
    match role {
        InternalRole::User => "user",
        InternalRole::Assistant => "assistant",
        InternalRole::System => "system",
        InternalRole::Tool => "tool",
    }
}
