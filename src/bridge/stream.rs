use std::{collections::VecDeque, pin::Pin};

use axum::body::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};

pub fn chat_sse_response_to_responses_sse(
    response: reqwest::Response,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    let upstream = response.bytes_stream();
    let state = ChatSseStreamState {
        upstream: Box::pin(upstream),
        decoder: ChatSseDecoder::default(),
        pending: VecDeque::new(),
        upstream_done: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(chunk) = state.pending.pop_front() {
                return Some((Ok(chunk), state));
            }
            if state.upstream_done {
                return None;
            }

            match state.upstream.next().await {
                Some(Ok(chunk)) => {
                    state.pending.extend(state.decoder.push_chunk(&chunk));
                }
                Some(Err(error)) => {
                    return Some((Err(std::io::Error::other(error)), state));
                }
                None => {
                    state.upstream_done = true;
                    state.pending.extend(state.decoder.finish());
                }
            }
        }
    })
}

pub fn chat_sse_response_to_anthropic_messages_sse(
    response: reqwest::Response,
    model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    let upstream = response.bytes_stream();
    let state = AnthropicMessagesSseStreamState {
        upstream: Box::pin(upstream),
        decoder: AnthropicMessagesSseDecoder::new(model),
        pending: VecDeque::new(),
        upstream_done: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(chunk) = state.pending.pop_front() {
                return Some((Ok(chunk), state));
            }
            if state.upstream_done {
                return None;
            }

            match state.upstream.next().await {
                Some(Ok(chunk)) => {
                    state.pending.extend(state.decoder.push_chunk(&chunk));
                }
                Some(Err(error)) => {
                    return Some((Err(std::io::Error::other(error)), state));
                }
                None => {
                    state.upstream_done = true;
                    state.pending.extend(state.decoder.finish());
                }
            }
        }
    })
}

struct ChatSseStreamState {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    decoder: ChatSseDecoder,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
}

#[derive(Default)]
struct ChatSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    completed: bool,
}

impl ChatSseDecoder {
    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.line_buffer.extend_from_slice(chunk);
        let mut output = Vec::new();

        while let Some(newline) = self.line_buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = self.line_buffer.drain(..=newline).collect::<Vec<_>>();
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            output.extend(self.process_line(&line));
        }

        output
    }

    fn finish(&mut self) -> Vec<Bytes> {
        let mut output = Vec::new();
        if !self.line_buffer.is_empty() {
            let line = std::mem::take(&mut self.line_buffer);
            if let Some(chunk) = self.process_line(&line) {
                output.push(chunk);
            }
            if let Some(chunk) = self.flush_event() {
                output.push(chunk);
            }
        }
        if !self.completed {
            self.completed = true;
            output.push(responses_completed_sse());
        }
        output
    }

    fn process_line(&mut self, line: &[u8]) -> Option<Bytes> {
        if line.is_empty() {
            return self.flush_event();
        }

        let line = std::str::from_utf8(line).ok()?;
        let data = line.strip_prefix("data:")?;
        let data = data.strip_prefix(' ').unwrap_or(data);
        self.data_lines.push(data.to_string());
        None
    }

    fn flush_event(&mut self) -> Option<Bytes> {
        if self.data_lines.is_empty() {
            return None;
        }

        let data = std::mem::take(&mut self.data_lines).join("\n");
        if data.trim() == "[DONE]" {
            self.completed = true;
            return Some(responses_completed_sse());
        }

        decode_chat_sse_text_delta(&data).map(|delta| responses_text_delta_sse(&delta))
    }
}

struct AnthropicMessagesSseStreamState {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    decoder: AnthropicMessagesSseDecoder,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
}

struct AnthropicMessagesSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    completed: bool,
    message_started: bool,
    content_block_started: bool,
    message_id: String,
    model: String,
}

impl AnthropicMessagesSseDecoder {
    fn new(model: String) -> Self {
        Self {
            line_buffer: Vec::new(),
            data_lines: Vec::new(),
            completed: false,
            message_started: false,
            content_block_started: false,
            message_id: format!("msg_{}", uuid::Uuid::new_v4()),
            model,
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.line_buffer.extend_from_slice(chunk);
        let mut output = Vec::new();

        while let Some(newline) = self.line_buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = self.line_buffer.drain(..=newline).collect::<Vec<_>>();
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
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
        if data.trim() == "[DONE]" {
            return vec![self.finish_message()];
        }

        decode_chat_sse_text_delta(&data)
            .map(|delta| vec![self.text_delta(&delta)])
            .unwrap_or_default()
    }

    fn text_delta(&mut self, delta: &str) -> Bytes {
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

    fn finish_message(&mut self) -> Bytes {
        self.completed = true;
        let mut chunk = String::new();
        if !self.message_started {
            self.message_started = true;
            chunk.push_str(&anthropic_message_start_sse(&self.message_id, &self.model));
        }
        if self.content_block_started {
            chunk.push_str(&anthropic_content_block_stop_sse());
        }
        chunk.push_str(&anthropic_message_delta_sse());
        chunk.push_str(&anthropic_message_stop_sse());
        Bytes::from(chunk)
    }
}

pub fn responses_text_delta_sse(delta: &str) -> Bytes {
    Bytes::from(format!(
        "event: response.output_text.delta\ndata: {delta}\n\n"
    ))
}

pub fn responses_completed_sse() -> Bytes {
    Bytes::from_static(b"event: response.completed\ndata: {}\n\ndata: [DONE]\n\n")
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
    anthropic_sse_event(
        "content_block_stop",
        json!({
            "type": "content_block_stop",
            "index": 0
        }),
    )
}

fn anthropic_message_delta_sse() -> String {
    anthropic_sse_event(
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn",
                "stop_sequence": null
            },
            "usage": {
                "output_tokens": 0
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

fn decode_chat_sse_text_delta(data: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{
        responses_completed_sse, responses_text_delta_sse, AnthropicMessagesSseDecoder,
        ChatSseDecoder,
    };

    #[test]
    fn decodes_chat_sse_events_split_across_chunks() {
        let mut decoder = ChatSseDecoder::default();

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
        let mut decoder = ChatSseDecoder::default();

        assert!(decoder
            .push_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}")
            .is_empty());
        let chunks = decoder.finish();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], responses_text_delta_sse("hel"));
        assert_eq!(chunks[1], responses_completed_sse());
    }

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
}
