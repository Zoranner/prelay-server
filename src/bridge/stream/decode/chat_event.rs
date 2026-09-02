use serde_json::Value;

use crate::bridge::stream::events::{
    internal_finish_reason_from_str, ChatSseEvent, ChatToolCallDelta,
};
use crate::bridge::stream::StreamUsage;

pub(crate) fn decode_chat_sse_event(data: &str) -> Option<ChatSseEvent> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    let usage = value.get("usage").map(|usage| StreamUsage {
        input_tokens: usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .and_then(Value::as_u64),
        output_tokens: usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
        cache_read_tokens: usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
            .or_else(|| usage.get("cache_read_input_tokens"))
            .and_then(Value::as_u64),
        cache_write_tokens: usage
            .pointer("/prompt_tokens_details/cache_write_tokens")
            .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
            .or_else(|| usage.get("cache_creation_input_tokens"))
            .and_then(Value::as_u64),
    });
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
    if choice.is_none() {
        return usage.map(|usage| ChatSseEvent {
            text_delta: None,
            tool_call_deltas: Vec::new(),
            finish_reason: None,
            usage: Some(usage),
        });
    }
    let choice = choice?;
    let delta = choice.get("delta");
    let text_delta = delta
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
        .or_else(|| {
            delta
                .and_then(|delta| delta.get("refusal"))
                .and_then(Value::as_str)
        })
        .map(str::to_string);
    let tool_call_deltas = delta
        .and_then(|delta| delta.get("tool_calls"))
        .and_then(Value::as_array)
        .map(|tool_calls| {
            tool_calls
                .iter()
                .filter_map(decode_chat_tool_call_delta)
                .collect()
        })
        .unwrap_or_default();
    let finish_reason = choice
        .get("finish_reason")
        .filter(|finish_reason| !finish_reason.is_null())
        .and_then(|finish_reason| {
            let reason = finish_reason.as_str();
            if reason.is_none_or(str::is_empty) {
                None
            } else {
                Some(internal_finish_reason_from_str(reason))
            }
        });

    Some(ChatSseEvent {
        text_delta,
        tool_call_deltas,
        finish_reason,
        usage,
    })
}

fn decode_chat_tool_call_delta(value: &Value) -> Option<ChatToolCallDelta> {
    let index = value.get("index").and_then(Value::as_u64)? as usize;
    let id = value.get("id").and_then(Value::as_str).map(str::to_string);
    let function = value.get("function");
    let name = function
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let arguments = function
        .and_then(|function| function.get("arguments"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(ChatToolCallDelta {
        index,
        id,
        name,
        arguments,
    })
}

#[cfg(test)]
mod tests {
    use super::decode_chat_sse_event;

    #[test]
    fn maps_chat_sse_finish_reason_to_internal_event() {
        let event =
            decode_chat_sse_event(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#)
                .expect("chat sse event");

        assert!(event.finished());
        assert!(event.to_internal_events().iter().any(|event| {
            matches!(
                event,
                crate::bridge::stream::InternalStreamEvent::Finished(
                    crate::bridge::stream::InternalFinishReason::ToolUse
                )
            )
        }));
    }
}
