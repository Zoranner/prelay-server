use axum::body::Bytes;
use serde_json::Value;

use super::{
    events::AnthropicMessagesSseEvent, pipeline::ByteStreamDecoder, responses_completed_sse,
    responses_text_delta_sse, sse::drain_lines,
};

#[derive(Default)]
pub(crate) struct AnthropicMessagesToResponsesSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    completed: bool,
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

        let Some(event) = decode_anthropic_messages_sse_event(&data) else {
            return Vec::new();
        };

        let mut output = Vec::new();
        if let Some(delta) = event.text_delta {
            output.push(responses_text_delta_sse(&delta));
        }
        if event.finished {
            self.completed = true;
            output.push(responses_completed_sse());
        }
        output
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
            output.push(responses_completed_sse());
        }
        output
    }
}

fn decode_anthropic_messages_sse_event(data: &str) -> Option<AnthropicMessagesSseEvent> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    let event_type = value.get("type").and_then(Value::as_str);
    let text_delta = value
        .get("delta")
        .filter(|_| event_type == Some("content_block_delta"))
        .filter(|delta| delta.get("type").and_then(Value::as_str) == Some("text_delta"))
        .and_then(|delta| delta.get("text"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let finished = event_type == Some("message_stop")
        || (event_type == Some("message_delta")
            && value
                .pointer("/delta/stop_reason")
                .is_some_and(|stop_reason| !stop_reason.is_null()));

    Some(AnthropicMessagesSseEvent {
        text_delta,
        finished,
    })
}

#[cfg(test)]
mod tests {
    use super::AnthropicMessagesToResponsesSseDecoder;
    use crate::bridge::stream::{
        pipeline::ByteStreamDecoder, responses_completed_sse, responses_text_delta_sse,
    };

    #[test]
    fn decodes_anthropic_messages_sse_text_delta_to_responses_sse() {
        let mut decoder = AnthropicMessagesToResponsesSseDecoder::default();

        let chunks = decoder.push_chunk(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
        );

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], responses_text_delta_sse("hel"));
    }

    #[test]
    fn decodes_anthropic_messages_sse_stop_to_responses_completed() {
        let mut decoder = AnthropicMessagesToResponsesSseDecoder::default();

        let chunks = decoder.push_chunk(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n\
              event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], responses_text_delta_sse("hel"));
        assert_eq!(chunks[1], responses_completed_sse());
    }
}
