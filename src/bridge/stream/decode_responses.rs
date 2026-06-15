use axum::body::Bytes;
use serde_json::Value;

use super::{
    encode_anthropic::AnthropicMessagesSseDecoder, pipeline::ByteStreamDecoder, sse::drain_lines,
};

pub(crate) struct ResponsesSseAnthropicMessagesSseDecoder {
    line_buffer: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
    anthropic: AnthropicMessagesSseDecoder,
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

        match event_name.as_deref() {
            Some("response.output_text.delta") => decode_responses_text_delta(&data)
                .map(|delta| vec![self.anthropic.text_delta(&delta)])
                .unwrap_or_default(),
            Some("response.output_item.done") | Some("response.completed") => {
                vec![self.anthropic.finish_message()]
            }
            _ => Vec::new(),
        }
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

#[cfg(test)]
mod tests {
    use super::ResponsesSseAnthropicMessagesSseDecoder;
    use crate::bridge::stream::pipeline::ByteStreamDecoder;

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
}
