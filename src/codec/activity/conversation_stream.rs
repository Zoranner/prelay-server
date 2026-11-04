use std::collections::HashSet;

use serde_json::Value;

use crate::{
    codec::bridge::stream::StreamStatsSnapshot,
    domain::activity::{
        redact_sensitive_text, ContentDraft, ContentEventBody, ConversationMessage,
        ConversationReasoning, ConversationToolCall, ConversationUsage, EventPhase,
        RawStreamContentCapture, RawStreamProtocol,
    },
};

use super::conversation::{omitted_media, response_item};

pub fn stream_capture(capture: &RawStreamContentCapture) -> Vec<ContentDraft> {
    if capture.protocol() == RawStreamProtocol::ImageGeneration {
        return if capture.is_completed() {
            vec![ContentDraft::new(ContentEventBody::Message(
                ConversationMessage {
                    phase: EventPhase::Response,
                    role: "assistant".to_string(),
                    message_phase: None,
                    content: Value::Array(vec![omitted_media("output_image")]),
                },
            ))]
        } else {
            Vec::new()
        };
    }
    let mut events = Vec::new();
    if !capture.reasoning_text().is_empty() {
        events.push(ContentDraft::new(ContentEventBody::Reasoning(
            ConversationReasoning {
                phase: EventPhase::Response,
                summary: None,
                content: Some(redact_sensitive_text(capture.reasoning_text())),
            },
        )));
    }
    if !capture.output_text().is_empty() {
        events.push(ContentDraft::new(ContentEventBody::Message(
            ConversationMessage {
                phase: EventPhase::Response,
                role: "assistant".to_string(),
                message_phase: None,
                content: Value::String(redact_sensitive_text(capture.output_text())),
            },
        )));
    }
    for (id, name, arguments) in capture.captured_tool_calls() {
        events.push(ContentDraft::new(ContentEventBody::ToolCall(
            ConversationToolCall {
                phase: EventPhase::Response,
                tool_call_id: id,
                name,
                arguments: Value::String(arguments),
            },
        )));
    }
    events
}

pub fn stream_snapshot(snapshot: &StreamStatsSnapshot) -> Vec<ContentDraft> {
    let mut events = snapshot
        .response_output_items
        .iter()
        .flat_map(|item| response_item(item, EventPhase::Response))
        .collect::<Vec<_>>();
    let mut has_message = false;
    let mut has_reasoning = false;
    let mut tool_call_ids = HashSet::new();
    for event in &events {
        match &event.body {
            ContentEventBody::Message(_) => has_message = true,
            ContentEventBody::Reasoning(_) => has_reasoning = true,
            ContentEventBody::ToolCall(tool_call) => {
                tool_call_ids.insert(tool_call.tool_call_id.clone());
            }
            _ => {}
        }
    }
    if !has_message && !snapshot.output_text.is_empty() {
        events.push(ContentDraft::new(ContentEventBody::Message(
            ConversationMessage {
                phase: EventPhase::Response,
                role: "assistant".to_string(),
                message_phase: None,
                content: Value::String(redact_sensitive_text(&snapshot.output_text)),
            },
        )));
    }
    if !has_reasoning && !snapshot.reasoning_text.is_empty() {
        events.push(ContentDraft::new(ContentEventBody::Reasoning(
            ConversationReasoning {
                phase: EventPhase::Response,
                summary: None,
                content: Some(redact_sensitive_text(&snapshot.reasoning_text)),
            },
        )));
    }
    for (id, name, arguments) in snapshot.captured_tool_calls() {
        if tool_call_ids.insert(id.clone()) {
            events.push(ContentDraft::new(ContentEventBody::ToolCall(
                ConversationToolCall {
                    phase: EventPhase::Response,
                    tool_call_id: id,
                    name,
                    arguments: Value::String(arguments),
                },
            )));
        }
    }
    if snapshot.input_tokens.is_some()
        || snapshot.output_tokens.is_some()
        || snapshot.cache_read_tokens.is_some()
        || snapshot.cache_write_tokens.is_some()
    {
        events.push(ContentDraft::new(ContentEventBody::Usage(
            ConversationUsage {
                input_tokens: snapshot.input_tokens,
                output_tokens: snapshot.output_tokens,
                reasoning_tokens: None,
                cache_read_tokens: snapshot.cache_read_tokens,
                cache_write_tokens: snapshot.cache_write_tokens,
            },
        )));
    }
    events
}
