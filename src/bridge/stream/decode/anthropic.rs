use std::collections::{BTreeMap, BTreeSet};

use axum::body::Bytes;
use serde_json::Value;

use super::super::{
    encode::responses::{
        responses_function_call_added_sse, responses_function_call_arguments_delta_sse,
        responses_function_call_arguments_done_sse, responses_output_item_done_sse,
    },
    events::{
        AnthropicMessagesSseEvent, ChatToolCallDelta, ChatToolCallState, InternalFinishReason,
        InternalStreamEvent, StreamUsage,
    },
    pipeline::{ByteStreamDecoder, SharedStreamStats},
    responses_completed_sse, responses_text_delta_sse,
    sse::drain_lines,
};

#[derive(Default)]
pub(crate) struct AnthropicMessagesToResponsesSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    tool_calls: BTreeMap<usize, ChatToolCallState>,
    tool_block_indexes: BTreeSet<usize>,
    usage: Option<StreamUsage>,
    completed: bool,
    stats: Option<SharedStreamStats>,
}

impl AnthropicMessagesToResponsesSseDecoder {
    fn process_line(&mut self, line: &[u8]) -> Vec<Bytes> {
        if line.is_empty() {
            return self.flush_event();
        }

        let Ok(line) = std::str::from_utf8(line) else {
            return Vec::new();
        };
        let Some(data) = line.strip_prefix("data:") else {
            return Vec::new();
        };
        let data = data.strip_prefix(' ').unwrap_or(data);
        self.data_lines.push(data.to_string());
        Vec::new()
    }

    fn flush_event(&mut self) -> Vec<Bytes> {
        if self.data_lines.is_empty() {
            return Vec::new();
        }

        let data = std::mem::take(&mut self.data_lines).join("\n");
        if self.completed {
            return Vec::new();
        }
        if data.trim() == "[DONE]" {
            self.completed = true;
            return vec![responses_completed_sse()];
        }

        let mut output = Vec::new();
        for event in self.decode_sse_event(&data) {
            self.record_internal_event(&event);
            output.extend(self.internal_event_to_responses_sse(event));
        }
        output
    }

    fn decode_sse_event(&mut self, data: &str) -> Vec<InternalStreamEvent> {
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return Vec::new();
        };
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            return Vec::new();
        };

        match event_type {
            "content_block_start" => {
                let event = decode_anthropic_tool_content_block_start(&value);
                if event.is_some() {
                    self.tool_block_indexes
                        .insert(anthropic_content_block_index(&value));
                }
                event.into_iter().collect()
            }
            "content_block_stop" => {
                let index = anthropic_content_block_index(&value);
                if self.tool_block_indexes.remove(&index) {
                    decode_anthropic_tool_content_block_stop(&value)
                        .into_iter()
                        .collect()
                } else {
                    Vec::new()
                }
            }
            _ => decode_anthropic_messages_sse_event(data),
        }
    }

    fn internal_event_to_responses_sse(&mut self, event: InternalStreamEvent) -> Vec<Bytes> {
        match event {
            InternalStreamEvent::TextDelta(delta) => vec![responses_text_delta_sse(&delta)],
            InternalStreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => self.tool_call_delta_to_responses_sse(ChatToolCallDelta {
                index,
                id,
                name,
                arguments,
            }),
            InternalStreamEvent::ToolCallDone {
                index,
                id,
                name,
                arguments,
            } => self.tool_call_done_to_responses_sse(index, id, name, arguments),
            InternalStreamEvent::Usage(usage) => {
                self.usage = Some(usage);
                Vec::new()
            }
            InternalStreamEvent::Finished(_) => {
                self.completed = true;
                vec![responses_completed_sse()]
            }
        }
    }

    fn tool_call_delta_to_responses_sse(&mut self, delta: ChatToolCallDelta) -> Vec<Bytes> {
        let state = self.tool_calls.entry(delta.index).or_default();
        let mut output = Vec::new();

        if let Some(id) = delta.id {
            state.id = id;
        }
        if let Some(name) = delta.name {
            state.name = name;
        }

        if !state.added {
            state.added = true;
            output.push(responses_function_call_added_sse(delta.index, state));
        }

        if let Some(arguments) = delta.arguments {
            state.arguments.push_str(&arguments);
            output.push(responses_function_call_arguments_delta_sse(
                delta.index,
                state,
                &arguments,
            ));
        }

        output
    }

    fn tool_call_done_to_responses_sse(
        &mut self,
        index: usize,
        id: String,
        name: String,
        arguments: String,
    ) -> Vec<Bytes> {
        let state = self.tool_calls.entry(index).or_default();
        if !id.is_empty() {
            state.id = id;
        }
        if !name.is_empty() {
            state.name = name;
        }
        if !arguments.is_empty() {
            state.arguments = arguments;
        }
        if state.done {
            return Vec::new();
        }
        state.done = true;

        let mut output = Vec::new();
        if !state.added {
            state.added = true;
            output.push(responses_function_call_added_sse(index, state));
        }
        output.push(responses_function_call_arguments_done_sse(index, state));
        output.push(responses_output_item_done_sse(index, state));
        output
    }

    fn record_internal_event(&self, event: &InternalStreamEvent) {
        let Some(stats) = &self.stats else {
            return;
        };
        if let Ok(mut stats) = stats.lock() {
            stats.record_event(event);
        }
    }
}

impl ByteStreamDecoder for AnthropicMessagesToResponsesSseDecoder {
    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.line_buffer.extend_from_slice(chunk);
        let mut output = Vec::new();

        for line in drain_lines(&mut self.line_buffer) {
            output.extend(self.process_line(&line));
        }

        output
    }

    fn finish(&mut self) -> Vec<Bytes> {
        let mut output = Vec::new();
        if !self.line_buffer.is_empty() {
            let line = std::mem::take(&mut self.line_buffer);
            output.extend(self.process_line(&line));
            output.extend(self.flush_event());
        }
        if !self.completed {
            self.completed = true;
            self.record_internal_event(&InternalStreamEvent::Finished(InternalFinishReason::Stop));
            output.push(responses_completed_sse());
        }
        output
    }

    fn set_stats(&mut self, stats: SharedStreamStats) {
        self.stats = Some(stats);
    }
}

fn decode_anthropic_messages_sse_event(data: &str) -> Vec<InternalStreamEvent> {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    let Some(event_type) = value.get("type").and_then(Value::as_str) else {
        return Vec::new();
    };

    match event_type {
        "content_block_start" => decode_anthropic_tool_content_block_start(&value)
            .into_iter()
            .collect(),
        "content_block_delta" => decode_anthropic_content_block_delta(&value)
            .into_iter()
            .collect(),
        "content_block_stop" => Vec::new(),
        "message_delta" => {
            let mut events = Vec::new();
            if let Some(usage) = decode_anthropic_usage(&value) {
                events.push(InternalStreamEvent::Usage(usage));
            }
            let event = decode_anthropic_message_delta_or_stop_event(event_type, &value);
            if let Some(reason) = event.finish_reason {
                events.push(InternalStreamEvent::Finished(reason));
            }
            events
        }
        "message_stop" => vec![InternalStreamEvent::Finished(InternalFinishReason::Stop)],
        _ => Vec::new(),
    }
}

fn decode_anthropic_content_block_delta(value: &Value) -> Option<InternalStreamEvent> {
    let delta = value.get("delta")?;
    match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => delta
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .map(InternalStreamEvent::TextDelta),
        Some("input_json_delta") => decode_anthropic_tool_content_block_delta(value),
        _ => None,
    }
}

fn decode_anthropic_message_delta_or_stop_event(
    event_type: &str,
    value: &Value,
) -> AnthropicMessagesSseEvent {
    let stop_reason = (event_type == "message_delta")
        .then(|| value.pointer("/delta/stop_reason"))
        .flatten()
        .filter(|stop_reason| !stop_reason.is_null())
        .and_then(Value::as_str);

    AnthropicMessagesSseEvent {
        finish_reason: stop_reason
            .map(|reason| super::super::events::internal_finish_reason_from_str(Some(reason))),
    }
}

fn decode_anthropic_tool_content_block_start(value: &Value) -> Option<InternalStreamEvent> {
    let content_block = value.get("content_block")?;
    if content_block.get("type").and_then(Value::as_str) != Some("tool_use") {
        return None;
    }

    Some(InternalStreamEvent::ToolCallDelta {
        index: anthropic_content_block_index(value),
        id: content_block
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string),
        name: content_block
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        arguments: None,
    })
}

fn decode_anthropic_tool_content_block_delta(value: &Value) -> Option<InternalStreamEvent> {
    let delta = value.get("delta")?;
    if delta.get("type").and_then(Value::as_str) != Some("input_json_delta") {
        return None;
    }

    Some(InternalStreamEvent::ToolCallDelta {
        index: anthropic_content_block_index(value),
        id: None,
        name: None,
        arguments: delta
            .get("partial_json")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn decode_anthropic_tool_content_block_stop(value: &Value) -> Option<InternalStreamEvent> {
    Some(InternalStreamEvent::ToolCallDone {
        index: anthropic_content_block_index(value),
        id: String::new(),
        name: String::new(),
        arguments: String::new(),
    })
}

fn decode_anthropic_usage(value: &Value) -> Option<StreamUsage> {
    let usage = value.get("usage")?;
    Some(StreamUsage {
        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
        total_tokens: None,
        cache_read_tokens: usage.get("cache_read_input_tokens").and_then(Value::as_u64),
        cache_write_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64),
    })
}

fn anthropic_content_block_index(value: &Value) -> usize {
    value
        .get("index")
        .and_then(Value::as_u64)
        .unwrap_or_default() as usize
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
