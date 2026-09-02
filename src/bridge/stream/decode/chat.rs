use std::collections::BTreeMap;

use axum::body::Bytes;
use serde_json::Value;

use super::super::{
    encode::responses::{
        responses_function_call_added_sse, responses_function_call_arguments_delta_sse,
        responses_function_call_arguments_done_sse, responses_output_item_done_sse,
    },
    events::{
        internal_finish_reason_from_str, ChatSseEvent, ChatToolCallDelta, ChatToolCallState,
        InternalFinishReason,
    },
    pipeline::{ByteStreamDecoder, SharedStreamStats},
    responses_completed_sse_with_usage, responses_text_delta_sse,
    sse::drain_lines,
    InternalStreamEvent, StreamUsage,
};

#[derive(Default)]
pub(crate) struct ChatToResponsesSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    tool_calls: BTreeMap<usize, ChatToolCallState>,
    completed: bool,
    usage: Option<StreamUsage>,
    finish_reason: Option<InternalFinishReason>,
    stats: Option<SharedStreamStats>,
}

impl ChatToResponsesSseDecoder {
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
            return self.finish_response();
        }

        let Some(event) = decode_chat_sse_event(&data) else {
            return Vec::new();
        };

        let mut output = Vec::new();
        for event in event.to_internal_events() {
            if let InternalStreamEvent::Usage(usage) = &event {
                self.usage = Some(usage.clone());
            }
            self.record_internal_event(&event);
            output.extend(self.internal_event_to_responses_sse(event));
        }
        output
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
            InternalStreamEvent::Finished(reason) => {
                self.finish_reason = Some(reason);
                Vec::new()
            }
            InternalStreamEvent::ToolCallDone { .. } | InternalStreamEvent::Usage(_) => Vec::new(),
        }
    }

    fn record_internal_event(&self, event: &InternalStreamEvent) {
        let Some(stats) = &self.stats else {
            return;
        };
        if let Ok(mut stats) = stats.lock() {
            stats.record_event(event);
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

    fn finish_response(&mut self) -> Vec<Bytes> {
        let mut output = Vec::new();
        let mut events_to_record = Vec::new();
        for (index, tool_call) in self.tool_calls.iter_mut() {
            if !tool_call.done {
                tool_call.done = true;
                let event = InternalStreamEvent::ToolCallDone {
                    index: *index,
                    id: tool_call.id.clone(),
                    name: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                };
                events_to_record.push(event.clone());
                output.extend(responses_tool_call_done_sse(event, tool_call));
            }
        }
        for event in events_to_record {
            self.record_internal_event(&event);
        }
        let usage_event = InternalStreamEvent::Usage(self.usage.clone().unwrap_or_default());
        self.record_internal_event(&usage_event);
        output.extend(self.internal_event_to_responses_sse(usage_event));
        let finished_event =
            InternalStreamEvent::Finished(self.finish_reason.unwrap_or(InternalFinishReason::Stop));
        self.record_internal_event(&finished_event);
        output.push(responses_completed_sse_with_usage(self.usage.as_ref()));
        output
    }
}

fn responses_tool_call_done_sse(
    event: InternalStreamEvent,
    tool_call: &ChatToolCallState,
) -> Vec<Bytes> {
    let InternalStreamEvent::ToolCallDone { index, .. } = event else {
        return Vec::new();
    };

    vec![
        responses_function_call_arguments_done_sse(index, tool_call),
        responses_output_item_done_sse(index, tool_call),
    ]
}

impl ByteStreamDecoder for ChatToResponsesSseDecoder {
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
            output.extend(self.finish_response());
        }
        output
    }

    fn set_stats(&mut self, stats: SharedStreamStats) {
        self.stats = Some(stats);
    }
}

#[derive(Default)]
pub(crate) struct ChatSseStatsDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    tool_calls: BTreeMap<usize, ChatToolCallState>,
    completed: bool,
    stats: Option<SharedStreamStats>,
}

impl ChatSseStatsDecoder {
    fn process_line(&mut self, line: &[u8]) {
        if line.is_empty() {
            self.flush_event();
            return;
        }
        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        if let Some(data) = line.strip_prefix("data:") {
            self.data_lines
                .push(data.strip_prefix(' ').unwrap_or(data).to_string());
        }
    }

    fn flush_event(&mut self) {
        if self.data_lines.is_empty() {
            return;
        }
        let data = std::mem::take(&mut self.data_lines).join("\n");
        if data.trim() == "[DONE]" {
            self.complete();
            return;
        }
        let Some(event) = decode_chat_sse_event(&data) else {
            return;
        };
        for event in event.to_internal_events() {
            if let InternalStreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } = &event
            {
                let state = self.tool_calls.entry(*index).or_default();
                if let Some(id) = id {
                    state.id = id.clone();
                }
                if let Some(name) = name {
                    state.name = name.clone();
                }
                if let Some(arguments) = arguments {
                    state.arguments.push_str(arguments);
                }
            }
            self.record(&event);
        }
    }

    fn complete(&mut self) {
        if self.completed {
            return;
        }
        self.completed = true;
        let done_events = self
            .tool_calls
            .iter_mut()
            .filter_map(|(index, state)| {
                if state.done {
                    return None;
                }
                state.done = true;
                Some(InternalStreamEvent::ToolCallDone {
                    index: *index,
                    id: state.id.clone(),
                    name: state.name.clone(),
                    arguments: state.arguments.clone(),
                })
            })
            .collect::<Vec<_>>();
        for event in done_events {
            self.record(&event);
        }
        self.record(&InternalStreamEvent::Finished(InternalFinishReason::Stop));
    }

    fn record(&self, event: &InternalStreamEvent) {
        if let Some(stats) = &self.stats {
            if let Ok(mut stats) = stats.lock() {
                stats.record_event(event);
            }
        }
    }
}

impl ByteStreamDecoder for ChatSseStatsDecoder {
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
        self.flush_event();
        if !self.completed {
            self.complete();
        }
        Vec::new()
    }

    fn set_stats(&mut self, stats: SharedStreamStats) {
        self.stats = Some(stats);
    }
}

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
    use std::sync::{Arc, Mutex};

    use super::ChatToResponsesSseDecoder;
    use crate::bridge::stream::{
        pipeline::ByteStreamDecoder, responses_completed_sse, responses_text_delta_sse,
    };
    use axum::body::Bytes;

    #[test]
    fn decodes_chat_sse_events_split_across_chunks() {
        let mut decoder = ChatToResponsesSseDecoder::default();

        assert!(decoder
            .push_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"he")
            .is_empty());
        let chunks = decoder.push_chunk(b"l\"}}]}\n\ndata: [DONE]\n\n");

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], responses_text_delta_sse("hel"));
        assert_eq!(chunks[1], responses_completed_sse());
    }

    #[test]
    fn finishes_trailing_event_without_blank_line() {
        let mut decoder = ChatToResponsesSseDecoder::default();

        assert!(decoder
            .push_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}")
            .is_empty());
        let chunks = decoder.finish();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], responses_text_delta_sse("hel"));
        assert_eq!(chunks[1], responses_completed_sse());
    }

    #[test]
    fn decodes_chat_sse_tool_call_to_responses_sse() {
        let mut decoder = ChatToResponsesSseDecoder::default();

        let chunks = decoder.push_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}]}}]}

data: [DONE]

"#,
        );

        assert_eq!(chunks.len(), 5);
        let output = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<String>();
        assert!(output.contains("event: response.output_item.added"));
        assert!(output.contains(r#""type":"function_call""#));
        assert!(output.contains(r#""id":"call_1""#));
        assert!(output.contains(r#""name":"get_weather""#));
        assert!(output.contains("event: response.function_call_arguments.delta"));
        assert!(output.contains(r#""delta":"{\"city\":\"Paris\"}""#));
        assert!(output.contains("event: response.function_call_arguments.done"));
        assert!(output.contains(r#""arguments":"{\"city\":\"Paris\"}""#));
        assert!(output.contains("event: response.output_item.done"));
        assert!(output.contains("event: response.completed"));
        assert!(output.contains("\"type\":\"response.completed\""));
        assert!(output.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn decodes_chat_sse_split_tool_call_arguments_to_responses_sse() {
        let mut decoder = ChatToResponsesSseDecoder::default();

        let first_chunks = decoder.push_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":"{\"city\":\"Par"}}]}}]}

"#,
        );
        let second_chunks = decoder.push_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"is\"}"}}]}}]}

data: [DONE]

"#,
        );

        assert_eq!(first_chunks.len(), 2);
        assert_eq!(second_chunks.len(), 4);
        let output = first_chunks
            .iter()
            .chain(second_chunks.iter())
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<String>();
        assert!(output.contains(r#""delta":"{\"city\":\"Par""#));
        assert!(output.contains(r#""delta":"is\"}""#));
        assert!(output.contains(r#""arguments":"{\"city\":\"Paris\"}""#));
    }

    #[test]
    fn maps_chat_sse_finish_reason_to_internal_event() {
        let event = super::decode_chat_sse_event(
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        )
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

    #[test]
    fn keeps_usage_chunk_received_after_finish_reason() {
        let mut decoder = ChatToResponsesSseDecoder::default();
        let chunks = decoder.push_chunk(
            br#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7,"prompt_tokens_details":{"cached_tokens":1,"cache_write_tokens":2}}}

data: [DONE]

"#,
        );

        let output = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<String>();
        assert!(output.contains(r#""input_tokens":3"#));
        assert!(output.contains(r#""output_tokens":4"#));
        assert!(output.contains(r#""cached_tokens":1"#));
        assert!(output.contains(r#""cache_write_tokens":2"#));
    }

    #[test]
    fn records_native_chat_stream_usage_without_changing_bytes() {
        let stats = Arc::new(Mutex::new(
            crate::bridge::stream::StreamStatsSnapshot::default(),
        ));
        let mut decoder = super::ChatSseStatsDecoder::default();
        super::ByteStreamDecoder::set_stats(&mut decoder, stats.clone());
        let chunk = br#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7,"prompt_tokens_details":{"cached_tokens":1,"cache_write_tokens":2}}}

data: [DONE]

"#;

        let output = super::ByteStreamDecoder::push_chunk(&mut decoder, chunk);
        assert_eq!(output, vec![Bytes::copy_from_slice(chunk)]);
        super::ByteStreamDecoder::finish(&mut decoder);

        let snapshot = stats.lock().expect("stream stats").clone();
        assert_eq!(snapshot.input_tokens, Some(3));
        assert_eq!(snapshot.output_tokens, Some(4));
        assert_eq!(snapshot.cache_read_tokens, Some(1));
        assert_eq!(snapshot.cache_write_tokens, Some(2));
        assert!(snapshot.completed);
    }
}
