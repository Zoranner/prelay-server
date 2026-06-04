use std::{collections::VecDeque, pin::Pin};

use axum::body::Bytes;
use futures::{Stream, StreamExt};
use serde_json::Value;

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

pub fn responses_text_delta_sse(delta: &str) -> Bytes {
    Bytes::from(format!(
        "event: response.output_text.delta\ndata: {delta}\n\n"
    ))
}

pub fn responses_completed_sse() -> Bytes {
    Bytes::from_static(b"event: response.completed\ndata: {}\n\ndata: [DONE]\n\n")
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
    use super::{responses_completed_sse, responses_text_delta_sse, ChatSseDecoder};

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
}
