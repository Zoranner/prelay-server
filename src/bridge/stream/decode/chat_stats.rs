use std::collections::BTreeMap;

use axum::body::Bytes;

use super::chat_event::decode_chat_sse_event;
use crate::bridge::stream::{
    events::{ChatToolCallState, InternalFinishReason},
    pipeline::{ByteStreamDecoder, SharedStreamStats},
    sse::drain_lines,
    InternalStreamEvent,
};

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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Bytes;

    use super::ChatSseStatsDecoder;
    use crate::bridge::stream::{pipeline::ByteStreamDecoder, StreamStatsSnapshot};

    #[test]
    fn records_native_chat_stream_usage_without_changing_bytes() {
        let stats = Arc::new(Mutex::new(StreamStatsSnapshot::default()));
        let mut decoder = ChatSseStatsDecoder::default();
        ByteStreamDecoder::set_stats(&mut decoder, stats.clone());
        let chunk = br#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7,"prompt_tokens_details":{"cached_tokens":1,"cache_write_tokens":2}}}

data: [DONE]

"#;

        let output = ByteStreamDecoder::push_chunk(&mut decoder, chunk);
        assert_eq!(output, vec![Bytes::copy_from_slice(chunk)]);
        ByteStreamDecoder::finish(&mut decoder);

        let snapshot = stats.lock().expect("stream stats").clone();
        assert_eq!(snapshot.input_tokens, Some(3));
        assert_eq!(snapshot.output_tokens, Some(4));
        assert_eq!(snapshot.cache_read_tokens, Some(1));
        assert_eq!(snapshot.cache_write_tokens, Some(2));
        assert!(snapshot.completed);
    }
}
