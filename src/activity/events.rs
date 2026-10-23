use crate::bridge::{
    internal::{
        InternalMessage, InternalOutputItem, InternalRequest, InternalResponse, InternalRole,
    },
    stream::StreamStatsSnapshot,
};
use serde_json::Value;

use super::{
    event_values::{chat_content_value, internal_content_value},
    ActivityMediaMetadata, EventPhase, MediaEvent, MessageEvent, RawStreamContentCapture,
    ReasoningEvent, RolloutEventBody, ToolCallEvent, ToolResultEvent, UsageEvent,
};

pub fn events_from_internal_request(request: &InternalRequest) -> Vec<RolloutEventBody> {
    let mut events = Vec::new();
    if let Some(instructions) = request
        .instructions
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        events.push(message_event(
            EventPhase::Request,
            "system",
            Value::String(instructions.to_string()),
        ));
    }
    for message in &request.messages {
        events.extend(internal_message_events(EventPhase::Request, message));
    }
    events
}

pub fn events_from_internal_response(response: &InternalResponse) -> Vec<RolloutEventBody> {
    let mut events = Vec::new();
    for output in &response.output {
        match output {
            InternalOutputItem::Message { content, .. } => {
                if let Some(value) = internal_content_value(content) {
                    events.push(message_event(EventPhase::Response, "assistant", value));
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
                    .filter(|value| !value.is_empty())
                {
                    events.push(reasoning_event(EventPhase::Response, reasoning));
                }
                events.push(tool_call_event(EventPhase::Response, id, name, arguments));
            }
        }
    }
    if let Some(usage) = &response.usage {
        events.push(usage_event(
            usage.input_tokens,
            usage.output_tokens,
            usage.reasoning_tokens,
            usage.cache_read_tokens,
            usage.cache_write_tokens,
        ));
    }
    events
}

pub fn events_from_chat_request(payload: &Value) -> Vec<RolloutEventBody> {
    payload
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|message| chat_message_events(EventPhase::Request, message))
        .collect()
}

pub fn events_from_chat_response(payload: &Value) -> Vec<RolloutEventBody> {
    let mut events = payload
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|choice| {
            choice
                .get("message")
                .map(|message| chat_message_events(EventPhase::Response, message))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    if let Some(usage) = payload.get("usage") {
        events.push(usage_event(
            usage.get("prompt_tokens").and_then(Value::as_i64),
            usage.get("completion_tokens").and_then(Value::as_i64),
            usage
                .get("completion_tokens_details")
                .and_then(|value| value.get("reasoning_tokens").and_then(Value::as_i64)),
            usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(Value::as_i64),
            usage
                .pointer("/prompt_tokens_details/cache_write_tokens")
                .and_then(Value::as_i64),
        ));
    }
    events
}

pub fn events_from_image_request(payload: &Value) -> Vec<RolloutEventBody> {
    payload
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|prompt| !prompt.is_empty())
        .map(|prompt| {
            vec![message_event(
                EventPhase::Request,
                "user",
                Value::String(prompt.to_string()),
            )]
        })
        .unwrap_or_default()
}

pub fn events_from_stream_capture(capture: &RawStreamContentCapture) -> Vec<RolloutEventBody> {
    let mut events = Vec::new();
    if !capture.reasoning_text().is_empty() {
        events.push(reasoning_event(
            EventPhase::Response,
            capture.reasoning_text(),
        ));
    }
    if !capture.output_text().is_empty() {
        events.push(message_event(
            EventPhase::Response,
            "assistant",
            Value::String(capture.output_text().to_string()),
        ));
    }
    for (id, name, arguments) in capture.captured_tool_calls() {
        events.push(tool_call_event(
            EventPhase::Response,
            &id,
            &name,
            &arguments,
        ));
    }
    events
}

pub fn events_from_stream_snapshot(snapshot: &StreamStatsSnapshot) -> Vec<RolloutEventBody> {
    let mut events = Vec::new();
    if !snapshot.reasoning_text.is_empty() {
        events.push(reasoning_event(
            EventPhase::Response,
            &snapshot.reasoning_text,
        ));
    }
    if !snapshot.output_text.is_empty() {
        events.push(message_event(
            EventPhase::Response,
            "assistant",
            Value::String(snapshot.output_text.clone()),
        ));
    }
    for (id, name, arguments) in snapshot.captured_tool_calls() {
        events.push(tool_call_event(
            EventPhase::Response,
            &id,
            &name,
            &arguments,
        ));
    }
    if snapshot.input_tokens.is_some()
        || snapshot.output_tokens.is_some()
        || snapshot.cache_read_tokens.is_some()
        || snapshot.cache_write_tokens.is_some()
    {
        events.push(usage_event(
            snapshot.input_tokens,
            snapshot.output_tokens,
            None,
            snapshot.cache_read_tokens,
            snapshot.cache_write_tokens,
        ));
    }
    events
}

pub fn media_event(phase: EventPhase, media: &ActivityMediaMetadata) -> RolloutEventBody {
    RolloutEventBody::Media(MediaEvent {
        phase,
        media_type: media.media_type.clone(),
        size_bytes: media.size_bytes as u64,
        sha256: media.sha256.clone(),
    })
}

fn internal_message_events(phase: EventPhase, message: &InternalMessage) -> Vec<RolloutEventBody> {
    let mut events = Vec::new();
    if let Some(content) = internal_content_value(&message.content) {
        if let Some(tool_call_id) = &message.tool_call_id {
            events.push(RolloutEventBody::ToolResult(ToolResultEvent {
                phase: phase.clone(),
                tool_call_id: tool_call_id.clone(),
                content,
            }));
        } else {
            events.push(message_event(
                phase.clone(),
                role_name(&message.role),
                content,
            ));
        }
    }
    if let Some(reasoning) = message
        .reasoning_content
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        events.push(reasoning_event(phase.clone(), reasoning));
    }
    events.extend(message.tool_calls.iter().map(|tool_call| {
        tool_call_event(
            phase.clone(),
            &tool_call.id,
            &tool_call.name,
            &tool_call.arguments,
        )
    }));
    events
}

fn chat_message_events(phase: EventPhase, message: &Value) -> Vec<RolloutEventBody> {
    let mut events = Vec::new();
    let content = message.get("content").and_then(chat_content_value);
    if let Some(content) = content {
        if let Some(tool_call_id) = message.get("tool_call_id").and_then(Value::as_str) {
            events.push(RolloutEventBody::ToolResult(ToolResultEvent {
                phase: phase.clone(),
                tool_call_id: tool_call_id.to_string(),
                content,
            }));
        } else {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user");
            events.push(message_event(phase.clone(), role, content));
        }
    }
    if let Some(reasoning) = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        events.push(reasoning_event(phase.clone(), reasoning));
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let id = tool_call.get("id").and_then(Value::as_str).unwrap_or("");
            let name = tool_call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = tool_call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("");
            events.push(tool_call_event(phase.clone(), id, name, arguments));
        }
    }
    events
}

fn role_name(role: &InternalRole) -> &'static str {
    match role {
        InternalRole::User => "user",
        InternalRole::Assistant => "assistant",
        InternalRole::System => "system",
        InternalRole::Tool => "tool",
    }
}

fn message_event(phase: EventPhase, role: &str, content: Value) -> RolloutEventBody {
    RolloutEventBody::Message(MessageEvent {
        phase,
        role: role.to_string(),
        content,
    })
}

fn tool_call_event(phase: EventPhase, id: &str, name: &str, arguments: &str) -> RolloutEventBody {
    RolloutEventBody::ToolCall(ToolCallEvent {
        phase,
        tool_call_id: id.to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
    })
}

fn reasoning_event(phase: EventPhase, content: &str) -> RolloutEventBody {
    RolloutEventBody::Reasoning(ReasoningEvent {
        phase,
        content: content.to_string(),
    })
}

fn usage_event(
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
) -> RolloutEventBody {
    RolloutEventBody::Usage(UsageEvent {
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_tokens,
        cache_write_tokens,
    })
}
