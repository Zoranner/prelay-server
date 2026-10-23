use axum::body::Bytes;
use serde_json::Value;

use super::super::{
    events::{InternalFinishReason, InternalStreamEvent, StreamUsage},
    pipeline::{ByteStreamDecoder, SharedStreamStats},
    sse::drain_lines,
};

#[derive(Default)]
pub(crate) struct NativeResponsesSseStatsDecoder {
    line_buffer: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
    stats: Option<SharedStreamStats>,
}

impl NativeResponsesSseStatsDecoder {
    fn process_line(&mut self, line: &[u8]) {
        if line.is_empty() {
            self.record_event();
            return;
        }

        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        if let Some(event) = line.strip_prefix("event:") {
            let event = event.strip_prefix(' ').unwrap_or(event);
            self.event_name = Some(event.to_string());
            return;
        }
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.strip_prefix(' ').unwrap_or(data);
        self.data_lines.push(data.to_string());
    }

    fn record_event(&mut self) {
        let event_name = self.event_name.take();
        let data = std::mem::take(&mut self.data_lines).join("\n");
        let Some(stats) = &self.stats else {
            return;
        };
        let Ok(mut stats) = stats.lock() else {
            return;
        };
        if let Some(response_id) = decode_responses_response_id(&data) {
            stats.response_id = Some(response_id);
        }
        for event in decode_responses_sse_event(event_name.as_deref(), &data) {
            stats.record_event(&event);
        }
    }
}

impl ByteStreamDecoder for NativeResponsesSseStatsDecoder {
    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.line_buffer.extend_from_slice(chunk);
        for line in drain_lines(&mut self.line_buffer) {
            self.process_line(&line);
        }
        vec![Bytes::copy_from_slice(chunk)]
    }

    fn finish(&mut self) -> Vec<Bytes> {
        if !self.line_buffer.is_empty() {
            let line = std::mem::take(&mut self.line_buffer);
            self.process_line(&line);
        }
        self.record_event();
        Vec::new()
    }

    fn set_stats(&mut self, stats: SharedStreamStats) {
        self.stats = Some(stats);
    }
}

fn decode_responses_text_delta(data: &str) -> Option<String> {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Some(data.to_string());
    };
    value
        .get("delta")
        .and_then(Value::as_str)
        .or_else(|| value.as_str())
        .map(str::to_string)
}

fn decode_responses_response_id(data: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    value
        .pointer("/response/id")
        .and_then(Value::as_str)
        .or_else(|| value.get("response_id").and_then(Value::as_str))
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
}

fn decode_responses_sse_event(event_name: Option<&str>, data: &str) -> Vec<InternalStreamEvent> {
    match event_name {
        Some("response.output_text.delta") => decode_responses_text_delta(data)
            .map(InternalStreamEvent::TextDelta)
            .into_iter()
            .collect(),
        Some("response.reasoning_summary_text.delta") | Some("response.reasoning_text.delta") => {
            decode_responses_text_delta(data)
                .map(InternalStreamEvent::ReasoningDelta)
                .into_iter()
                .collect()
        }
        Some("response.output_item.added") => decode_responses_function_call_added(data)
            .into_iter()
            .collect(),
        Some("response.function_call_arguments.delta") => {
            decode_responses_function_call_arguments_delta(data)
                .into_iter()
                .collect()
        }
        Some("response.function_call_arguments.done") => {
            decode_responses_function_call_arguments_done(data)
                .into_iter()
                .collect()
        }
        Some("response.output_item.done") => decode_responses_output_item_done(data)
            .into_iter()
            .collect(),
        Some("response.completed") => {
            let mut events = Vec::new();
            if let Some(usage) = decode_responses_usage(data) {
                events.push(InternalStreamEvent::Usage(usage));
            }
            events.push(InternalStreamEvent::Finished(InternalFinishReason::Stop));
            events
        }
        _ => Vec::new(),
    }
}

fn decode_responses_function_call_added(data: &str) -> Option<InternalStreamEvent> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    let item = value.get("item")?;
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return None;
    }
    let index = responses_output_index(&value);
    Some(InternalStreamEvent::ToolCallDelta {
        index,
        id: responses_tool_call_id(item, &value),
        name: item.get("name").and_then(Value::as_str).map(str::to_string),
        arguments: item
            .get("arguments")
            .and_then(Value::as_str)
            .filter(|arguments| !arguments.is_empty())
            .map(str::to_string),
    })
}

fn decode_responses_function_call_arguments_delta(data: &str) -> Option<InternalStreamEvent> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    Some(InternalStreamEvent::ToolCallDelta {
        index: responses_output_index(&value),
        id: responses_tool_call_id(&value, &value),
        name: None,
        arguments: value
            .get("delta")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn decode_responses_function_call_arguments_done(data: &str) -> Option<InternalStreamEvent> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    Some(InternalStreamEvent::ToolCallDone {
        index: responses_output_index(&value),
        id: responses_tool_call_id(&value, &value).unwrap_or_default(),
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        arguments: value
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn decode_responses_output_item_done(data: &str) -> Option<InternalStreamEvent> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    let item = value.get("item")?;
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return None;
    }
    Some(InternalStreamEvent::ToolCallDone {
        index: responses_output_index(&value),
        id: responses_tool_call_id(item, &value).unwrap_or_default(),
        name: item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        arguments: item
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn decode_responses_usage(data: &str) -> Option<StreamUsage> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    let usage = value
        .pointer("/response/usage")
        .or_else(|| value.get("usage"))?;
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_u64);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_u64);
    Some(StreamUsage {
        input_tokens,
        output_tokens,
        total_tokens: usage
            .get("total_tokens")
            .and_then(Value::as_u64)
            .or_else(|| {
                input_tokens
                    .zip(output_tokens)
                    .map(|(input, output)| input + output)
            }),
        cache_read_tokens: usage
            .pointer("/input_tokens_details/cached_tokens")
            .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
            .or_else(|| usage.get("cache_read_input_tokens"))
            .and_then(Value::as_u64),
        cache_write_tokens: usage
            .pointer("/input_tokens_details/cache_write_tokens")
            .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
            .or_else(|| usage.get("cache_creation_input_tokens"))
            .and_then(Value::as_u64),
    })
}

fn responses_output_index(value: &Value) -> usize {
    value
        .get("output_index")
        .and_then(Value::as_u64)
        .unwrap_or_default() as usize
}

fn responses_tool_call_id(primary: &Value, fallback: &Value) -> Option<String> {
    primary
        .get("call_id")
        .and_then(Value::as_str)
        .or_else(|| primary.get("id").and_then(Value::as_str))
        .or_else(|| fallback.get("call_id").and_then(Value::as_str))
        .or_else(|| fallback.get("item_id").and_then(Value::as_str))
        .map(str::to_string)
}

#[cfg(test)]
#[path = "responses_tests.rs"]
mod tests;
