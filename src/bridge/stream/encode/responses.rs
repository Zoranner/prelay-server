use axum::body::Bytes;
use serde_json::{json, Value};

use super::super::events::ChatToolCallState;
use super::super::events::StreamUsage;

pub fn responses_text_delta_sse(delta: &str) -> Bytes {
    responses_sse_event(
        "response.output_text.delta",
        json!({
            "type": "response.output_text.delta",
            "delta": delta,
            "output_index": 0,
            "content_index": 0,
            "item_id": "msg_0",
            "sequence_number": 0
        }),
    )
}

pub fn responses_completed_sse() -> Bytes {
    responses_completed_sse_with_usage(None)
}

pub fn responses_completed_sse_with_usage(usage: Option<&StreamUsage>) -> Bytes {
    let event = json!({
        "type": "response.completed",
        "response": {
            "id": "resp_unknown",
            "object": "response",
            "status": "completed",
            "output": [],
            "usage": usage.map(|usage| json!({
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "total_tokens": usage.total_tokens,
                "input_tokens_details": {
                    "cached_tokens": usage.cache_read_tokens,
                    "cache_write_tokens": usage.cache_write_tokens,
                }
            })),
            "sequence_number": 0
        }
    });
    Bytes::from(format!(
        "event: response.completed\ndata: {event}\n\ndata: [DONE]\n\n"
    ))
}

pub(crate) fn responses_function_call_added_sse(
    index: usize,
    tool_call: &ChatToolCallState,
) -> Bytes {
    responses_sse_event(
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "output_index": index,
            "item": {
                "type": "function_call",
                "id": tool_call.id,
                "call_id": tool_call.id,
                "name": tool_call.name,
                "arguments": ""
            },
            "sequence_number": 0
        }),
    )
}

pub(crate) fn responses_function_call_arguments_delta_sse(
    index: usize,
    tool_call: &ChatToolCallState,
    delta: &str,
) -> Bytes {
    responses_sse_event(
        "response.function_call_arguments.delta",
        json!({
            "type": "response.function_call_arguments.delta",
            "item_id": tool_call.id,
            "output_index": index,
            "call_id": tool_call.id,
            "delta": delta,
            "sequence_number": 0
        }),
    )
}

pub(crate) fn responses_function_call_arguments_done_sse(
    index: usize,
    tool_call: &ChatToolCallState,
) -> Bytes {
    responses_sse_event(
        "response.function_call_arguments.done",
        json!({
            "type": "response.function_call_arguments.done",
            "item_id": tool_call.id,
            "output_index": index,
            "call_id": tool_call.id,
            "name": tool_call.name,
            "arguments": tool_call.arguments,
            "sequence_number": 0
        }),
    )
}

pub(crate) fn responses_output_item_done_sse(index: usize, tool_call: &ChatToolCallState) -> Bytes {
    responses_sse_event(
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": index,
            "item": {
                "type": "function_call",
                "id": tool_call.id,
                "call_id": tool_call.id,
                "name": tool_call.name,
                "arguments": tool_call.arguments
            },
            "sequence_number": 0
        }),
    )
}

fn responses_sse_event(event: &str, data: Value) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
}
