use axum::body::Bytes;
use serde_json::Value;

use super::{
    encode_anthropic::AnthropicMessagesSseDecoder,
    events::{InternalFinishReason, InternalStreamEvent, StreamUsage},
    pipeline::{ByteStreamDecoder, SharedStreamStats},
    sse::drain_lines,
};

pub(crate) struct ResponsesSseAnthropicMessagesSseDecoder {
    line_buffer: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
    anthropic: AnthropicMessagesSseDecoder,
}

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
        for event in decode_responses_sse_event(event_name.as_deref(), &data) {
            stats.record_event(&event);
        }
    }
}

impl ResponsesSseAnthropicMessagesSseDecoder {
    pub(crate) fn new(model: String) -> Self {
        Self {
            line_buffer: Vec::new(),
            event_name: None,
            data_lines: Vec::new(),
            anthropic: AnthropicMessagesSseDecoder::new(model),
        }
    }

    fn process_line(&mut self, line: &[u8]) -> Vec<Bytes> {
        if line.is_empty() {
            return self.flush_event();
        }

        let Ok(line) = std::str::from_utf8(line) else {
            return Vec::new();
        };
        if let Some(event) = line.strip_prefix("event:") {
            let event = event.strip_prefix(' ').unwrap_or(event);
            self.event_name = Some(event.to_string());
            return Vec::new();
        }
        let Some(data) = line.strip_prefix("data:") else {
            return Vec::new();
        };
        let data = data.strip_prefix(' ').unwrap_or(data);
        self.data_lines.push(data.to_string());
        Vec::new()
    }

    fn flush_event(&mut self) -> Vec<Bytes> {
        let event_name = self.event_name.take();
        let data = std::mem::take(&mut self.data_lines).join("\n");
        if self.anthropic.completed {
            return Vec::new();
        }
        if data.trim() == "[DONE]" {
            return Vec::new();
        }

        let events = decode_responses_sse_event(event_name.as_deref(), &data);
        let mut output = Vec::new();
        for event in events {
            output.extend(self.anthropic.internal_event_to_anthropic_sse(event));
        }
        output
    }
}

impl ByteStreamDecoder for ResponsesSseAnthropicMessagesSseDecoder {
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
        if !self.anthropic.completed {
            output.push(self.anthropic.finish_message());
        }
        output
    }

    fn set_stats(&mut self, stats: SharedStreamStats) {
        self.anthropic.set_stats(stats);
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

fn decode_responses_sse_event(event_name: Option<&str>, data: &str) -> Vec<InternalStreamEvent> {
    match event_name {
        Some("response.output_text.delta") => decode_responses_text_delta(data)
            .map(InternalStreamEvent::TextDelta)
            .into_iter()
            .collect(),
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
            .get("cache_creation_input_tokens")
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
mod tests {
    use super::{decode_responses_usage, ResponsesSseAnthropicMessagesSseDecoder};
    use std::sync::{Arc, Mutex};

    use crate::bridge::stream::{pipeline::ByteStreamDecoder, StreamStatsSnapshot};

    #[test]
    fn decodes_responses_sse_text_delta_to_anthropic_messages_sse() {
        let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());

        let chunks =
            decoder.push_chunk(b"event: response.output_text.delta\ndata: {\"delta\":\"hel\"}\n\n");

        assert_eq!(chunks.len(), 1);
        let chunk = std::str::from_utf8(&chunks[0]).expect("utf8 chunk");
        assert!(chunk.contains("event: message_start"));
        assert!(chunk.contains("event: content_block_start"));
        assert!(chunk.contains("event: content_block_delta"));
        assert!(chunk.contains("\"text\":\"hel\""));
    }

    #[test]
    fn decodes_responses_sse_completion_to_anthropic_messages_stop() {
        let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());

        let chunks = decoder.push_chunk(
            b"event: response.output_text.delta\ndata: {\"delta\":\"hel\"}\n\n\
              event: response.completed\ndata: {}\n\n",
        );
        let output = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<String>();

        assert!(output.contains("event: content_block_delta"));
        assert!(output.contains("event: content_block_stop"));
        assert!(output.contains("event: message_delta"));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn decodes_responses_sse_function_call_to_anthropic_messages_tool_use() {
        let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());

        let chunks = decoder.push_chunk(
            br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather","arguments":""}}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","call_id":"call_1","delta":"{\"city\":\"Par"}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","call_id":"call_1","delta":"is\"}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","call_id":"call_1","name":"get_weather","arguments":"{\"city\":\"Paris\"}"}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}

event: response.completed
data: {"type":"response.completed","response":{"usage":{"input_tokens":3,"output_tokens":5,"total_tokens":8}}}

"#,
        );
        let output = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<String>();

        assert!(output.contains("event: message_start"));
        assert!(output.contains("event: content_block_start"));
        assert!(output.contains(r#""type":"tool_use""#));
        assert!(output.contains(r#""id":"call_1""#));
        assert!(output.contains(r#""name":"get_weather""#));
        assert!(output.contains("event: content_block_delta"));
        assert!(output.contains(r#""type":"input_json_delta""#));
        assert!(output.contains(r#""partial_json":"{\"city\":\"Par""#));
        assert!(output.contains(r#""partial_json":"is\"}""#));
        assert!(output.contains("event: content_block_stop"));
        assert!(output.contains(r#""stop_reason":"tool_use""#));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn records_responses_function_call_done_once_when_arguments_and_item_done_arrive() {
        let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());
        let stats = Arc::new(Mutex::new(StreamStatsSnapshot::default()));
        decoder.set_stats(stats.clone());

        let chunks = decoder.push_chunk(
            br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather","arguments":""}}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","call_id":"call_1","delta":"{}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","call_id":"call_1","name":"get_weather","arguments":"{}"}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather","arguments":"{}"}}

event: response.completed
data: {"type":"response.completed","response":{"usage":{"input_tokens":3,"output_tokens":5,"total_tokens":8}}}

"#,
        );
        let output = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<String>();
        let block_stop_count = output.matches("event: content_block_stop").count();
        let stats = stats.lock().expect("stats lock");

        assert_eq!(block_stop_count, 1);
        assert_eq!(stats.tool_call_count, 1);
        assert!(stats.completed);
    }

    #[test]
    fn ignores_unknown_responses_sse_event_and_continues() {
        let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());

        let chunks = decoder.push_chunk(
            b"event: response.unexpected\ndata: {\"type\":\"response.unexpected\"}\n\n\
              event: response.output_text.delta\ndata: {\"delta\":\"hel\"}\n\n",
        );
        let output = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<String>();

        assert!(output.contains("event: content_block_delta"));
        assert!(output.contains("\"text\":\"hel\""));
    }

    #[test]
    fn decodes_openai_named_usage_from_completed_responses_event() {
        let usage = decode_responses_usage(
            r#"{"response":{"usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}}"#,
        )
        .expect("usage");

        assert_eq!(usage.input_tokens, Some(11));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.total_tokens, Some(18));
    }
}
