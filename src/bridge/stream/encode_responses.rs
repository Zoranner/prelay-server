use axum::body::Bytes;
use serde_json::{json, Value};

use super::events::ChatToolCallState;

pub fn responses_text_delta_sse(delta: &str) -> Bytes {
    Bytes::from(format!(
        "event: response.output_text.delta\ndata: {delta}\n\n"
    ))
}

pub fn responses_completed_sse() -> Bytes {
    Bytes::from_static(b"event: response.completed\ndata: {}\n\ndata: [DONE]\n\n")
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
            }
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
            "delta": delta
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
            "arguments": tool_call.arguments
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
            }
        }),
    )
}

fn responses_sse_event(event: &str, data: Value) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
}
