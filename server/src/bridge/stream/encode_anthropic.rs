use std::collections::BTreeMap;

use axum::body::Bytes;
use serde_json::{json, Value};

use super::{
    decode_chat::decode_chat_sse_event,
    events::{
        ChatToolCallDelta, ChatToolCallState, InternalFinishReason, InternalStreamEvent,
        StreamUsage,
    },
    pipeline::{ByteStreamDecoder, SharedStreamStats},
    sse::drain_lines,
};

pub(crate) struct AnthropicMessagesSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    tool_calls: BTreeMap<usize, ChatToolCallState>,
    pub(crate) completed: bool,
    message_started: bool,
    content_block_started: bool,
    used_tool: bool,
    message_id: String,
    model: String,
    usage: Option<StreamUsage>,
    stats: Option<SharedStreamStats>,
}

impl AnthropicMessagesSseDecoder {
    pub(crate) fn new(model: String) -> Self {
        Self {
            line_buffer: Vec::new(),
            data_lines: Vec::new(),
            tool_calls: BTreeMap::new(),
            completed: false,
            message_started: false,
            content_block_started: false,
            used_tool: false,
            message_id: format!("msg_{}", uuid::Uuid::new_v4()),
            model,
            usage: None,
            stats: None,
        }
    }

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
            return vec![self.finish_message()];
        }

        let Some(event) = decode_chat_sse_event(&data) else {
            return Vec::new();
        };
        for event in event.to_internal_events() {
            self.record_internal_event(&event);
        }

        let mut output = Vec::new();
        if let Some(delta) = &event.text_delta {
            output.push(self.text_delta(delta));
        }
        let finished = event.finished();
        for delta in event.tool_call_deltas {
            output.extend(self.tool_call_delta(delta));
        }
        if finished {
            output.push(self.finish_message());
        }
        output
    }

    pub(crate) fn text_delta(&mut self, delta: &str) -> Bytes {
        let mut chunk = String::new();
        if !self.message_started {
            self.message_started = true;
            chunk.push_str(&anthropic_message_start_sse(&self.message_id, &self.model));
        }
        if !self.content_block_started {
            self.content_block_started = true;
            chunk.push_str(&anthropic_content_block_start_sse());
        }
        chunk.push_str(&anthropic_content_block_delta_sse(delta));
        Bytes::from(chunk)
    }

    pub(crate) fn internal_event_to_anthropic_sse(
        &mut self,
        event: InternalStreamEvent,
    ) -> Vec<Bytes> {
        match event {
            InternalStreamEvent::TextDelta(delta) => {
                self.record_internal_event(&InternalStreamEvent::TextDelta(delta.clone()));
                vec![self.text_delta(&delta)]
            }
            InternalStreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => {
                let event = InternalStreamEvent::ToolCallDelta {
                    index,
                    id,
                    name,
                    arguments,
                };
                self.record_internal_event(&event);
                let InternalStreamEvent::ToolCallDelta {
                    index,
                    id,
                    name,
                    arguments,
                } = event
                else {
                    return Vec::new();
                };
                self.tool_call_delta(ChatToolCallDelta {
                    index,
                    id,
                    name,
                    arguments,
                })
            }
            InternalStreamEvent::ToolCallDone {
                index,
                id,
                name,
                arguments,
            } => {
                if let Some(tool_call) = self.tool_calls.get_mut(&index) {
                    if tool_call.done {
                        Vec::new()
                    } else {
                        tool_call.done = true;
                        self.record_internal_event(&InternalStreamEvent::ToolCallDone {
                            index,
                            id,
                            name,
                            arguments,
                        });
                        vec![Bytes::from(anthropic_content_block_stop_at_index_sse(
                            index,
                        ))]
                    }
                } else {
                    Vec::new()
                }
            }
            InternalStreamEvent::Usage(usage) => {
                self.record_internal_event(&InternalStreamEvent::Usage(usage.clone()));
                self.usage = Some(usage);
                Vec::new()
            }
            InternalStreamEvent::Finished(reason) => {
                self.record_internal_event(&InternalStreamEvent::Finished(reason));
                vec![self.finish_message()]
            }
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

    fn tool_call_delta(&mut self, delta: ChatToolCallDelta) -> Vec<Bytes> {
        let state = self.tool_calls.entry(delta.index).or_default();
        let mut output = Vec::new();

        if let Some(id) = delta.id {
            state.id = id;
        }
        if let Some(name) = delta.name {
            state.name = name;
        }

        if !state.added {
            self.used_tool = true;
            state.added = true;
            let mut chunk = String::new();
            if !self.message_started {
                self.message_started = true;
                chunk.push_str(&anthropic_message_start_sse(&self.message_id, &self.model));
            }
            if self.content_block_started {
                self.content_block_started = false;
                chunk.push_str(&anthropic_content_block_stop_sse());
            }
            chunk.push_str(&anthropic_tool_content_block_start_sse(delta.index, state));
            output.push(Bytes::from(chunk));
        }

        if let Some(arguments) = delta.arguments {
            state.arguments.push_str(&arguments);
            output.push(Bytes::from(anthropic_tool_content_block_delta_sse(
                delta.index,
                &arguments,
            )));
        }

        output
    }

    pub(crate) fn finish_message(&mut self) -> Bytes {
        self.completed = true;
        let mut chunk = String::new();
        let mut events_to_record = Vec::new();
        if !self.message_started {
            self.message_started = true;
            chunk.push_str(&anthropic_message_start_sse(&self.message_id, &self.model));
        }
        if self.content_block_started {
            chunk.push_str(&anthropic_content_block_stop_sse());
        }
        for (index, tool_call) in self.tool_calls.iter_mut() {
            if !tool_call.done {
                tool_call.done = true;
                events_to_record.push(InternalStreamEvent::ToolCallDone {
                    index: *index,
                    id: tool_call.id.clone(),
                    name: tool_call.name.clone(),
                    arguments: tool_call.arguments.clone(),
                });
                chunk.push_str(&anthropic_content_block_stop_at_index_sse(*index));
            }
        }
        chunk.push_str(&anthropic_message_delta_sse(
            if self.used_tool {
                "tool_use"
            } else {
                "end_turn"
            },
            self.usage.as_ref(),
        ));
        chunk.push_str(&anthropic_message_stop_sse());
        for event in events_to_record {
            self.record_internal_event(&event);
        }
        self.record_internal_event(&InternalStreamEvent::Finished(if self.used_tool {
            InternalFinishReason::ToolUse
        } else {
            InternalFinishReason::Stop
        }));
        Bytes::from(chunk)
    }
}

impl ByteStreamDecoder for AnthropicMessagesSseDecoder {
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
            output.push(self.finish_message());
        }
        output
    }

    fn set_stats(&mut self, stats: SharedStreamStats) {
        self.stats = Some(stats);
    }
}

fn anthropic_message_start_sse(message_id: &str, model: &str) -> String {
    anthropic_sse_event(
        "message_start",
        json!({
            "type": "message_start",
            "message": {
                "id": message_id,
                "type": "message",
                "role": "assistant",
                "model": model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0
                }
            }
        }),
    )
}

fn anthropic_content_block_start_sse() -> String {
    anthropic_sse_event(
        "content_block_start",
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "text",
                "text": ""
            }
        }),
    )
}

fn anthropic_content_block_delta_sse(delta: &str) -> String {
    anthropic_sse_event(
        "content_block_delta",
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {
                "type": "text_delta",
                "text": delta
            }
        }),
    )
}

fn anthropic_content_block_stop_sse() -> String {
    anthropic_content_block_stop_at_index_sse(0)
}

fn anthropic_content_block_stop_at_index_sse(index: usize) -> String {
    anthropic_sse_event(
        "content_block_stop",
        json!({
            "type": "content_block_stop",
            "index": index
        }),
    )
}

fn anthropic_tool_content_block_start_sse(index: usize, tool_call: &ChatToolCallState) -> String {
    anthropic_sse_event(
        "content_block_start",
        json!({
            "type": "content_block_start",
            "index": index,
            "content_block": {
                "type": "tool_use",
                "id": tool_call.id,
                "name": tool_call.name,
                "input": {}
            }
        }),
    )
}

fn anthropic_tool_content_block_delta_sse(index: usize, delta: &str) -> String {
    anthropic_sse_event(
        "content_block_delta",
        json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "input_json_delta",
                "partial_json": delta
            }
        }),
    )
}

fn anthropic_message_delta_sse(stop_reason: &str, usage: Option<&StreamUsage>) -> String {
    let output_tokens = usage
        .and_then(|usage| usage.output_tokens)
        .unwrap_or_default();
    anthropic_sse_event(
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null
            },
            "usage": {
                "output_tokens": output_tokens
            }
        }),
    )
}

fn anthropic_message_stop_sse() -> String {
    anthropic_sse_event(
        "message_stop",
        json!({
            "type": "message_stop"
        }),
    )
}

fn anthropic_sse_event(event: &str, data: Value) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

#[cfg(test)]
mod tests {
    use super::AnthropicMessagesSseDecoder;
    use std::sync::{Arc, Mutex};

    use crate::bridge::stream::{pipeline::ByteStreamDecoder, StreamStatsSnapshot};

    #[test]
    fn decodes_chat_sse_text_delta_to_anthropic_messages_sse() {
        let mut decoder = AnthropicMessagesSseDecoder::new("deepseek-chat".to_string());

        let chunks =
            decoder.push_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n");

        assert_eq!(chunks.len(), 1);
        let chunk = std::str::from_utf8(&chunks[0]).expect("utf8 chunk");
        assert!(chunk.contains("event: message_start"));
        assert!(chunk.contains("event: content_block_start"));
        assert!(chunk.contains("event: content_block_delta"));
        assert!(chunk.contains("\"text\":\"hel\""));
    }

    #[test]
    fn decodes_chat_sse_tool_call_to_anthropic_messages_sse() {
        let mut decoder = AnthropicMessagesSseDecoder::new("deepseek-chat".to_string());

        let chunks = decoder.push_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}]}}]}

data: [DONE]

"#,
        );

        assert_eq!(chunks.len(), 3);
        let output = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<String>();
        assert!(output.contains("event: message_start"));
        assert!(output.contains("event: content_block_start"));
        assert!(output.contains(r#""type":"tool_use""#));
        assert!(output.contains(r#""id":"call_1""#));
        assert!(output.contains(r#""name":"get_weather""#));
        assert!(output.contains(r#""input":{}"#));
        assert!(output.contains("event: content_block_delta"));
        assert!(output.contains(r#""type":"input_json_delta""#));
        assert!(output.contains(r#""partial_json":"{\"city\":\"Paris\"}""#));
        assert!(output.contains("event: content_block_stop"));
        assert!(output.contains("event: message_delta"));
        assert!(output.contains(r#""stop_reason":"tool_use""#));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn records_tool_call_done_when_finish_message_closes_open_tool_block() {
        let mut decoder = AnthropicMessagesSseDecoder::new("deepseek-chat".to_string());
        let stats = Arc::new(Mutex::new(StreamStatsSnapshot::default()));
        decoder.set_stats(stats.clone());

        let chunks = decoder.push_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":"{}"}}]}}]}

data: [DONE]

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
}
