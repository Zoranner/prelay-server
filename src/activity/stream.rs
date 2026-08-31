use serde_json::Value;

const MAX_CAPTURED_STREAM_TEXT_BYTES: usize = 128 * 1024;
const MAX_CAPTURED_SSE_EVENT_BYTES: usize = MAX_CAPTURED_STREAM_TEXT_BYTES + 16 * 1024;

pub enum RawStreamProtocol {
    ChatCompletions,
    AnthropicMessages,
    ImageGeneration,
}

pub struct RawStreamContentCapture {
    protocol: RawStreamProtocol,
    line_buffer: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
    event_bytes: usize,
    discarding_event: bool,
    output_text: String,
    completed: bool,
    is_truncated: bool,
}

impl RawStreamContentCapture {
    pub fn new(protocol: RawStreamProtocol) -> Self {
        Self {
            protocol,
            line_buffer: Vec::new(),
            event_name: None,
            data_lines: Vec::new(),
            event_bytes: 0,
            discarding_event: false,
            output_text: String::new(),
            completed: false,
            is_truncated: false,
        }
    }

    pub fn observe_chunk(&mut self, mut chunk: &[u8]) {
        while !chunk.is_empty() {
            if self.discarding_event {
                let Some(newline) = chunk.iter().position(|byte| *byte == b'\n') else {
                    return;
                };
                let line = &chunk[..newline];
                chunk = &chunk[newline + 1..];
                if line.is_empty() || line == b"\r" {
                    self.discarding_event = false;
                }
                continue;
            }

            let Some(newline) = chunk.iter().position(|byte| *byte == b'\n') else {
                if self
                    .event_bytes
                    .saturating_add(self.line_buffer.len() + chunk.len())
                    > MAX_CAPTURED_SSE_EVENT_BYTES
                {
                    self.discard_current_event();
                } else {
                    self.line_buffer.extend_from_slice(chunk);
                }
                return;
            };
            let line = &chunk[..newline];
            chunk = &chunk[newline + 1..];
            if self
                .event_bytes
                .saturating_add(self.line_buffer.len() + line.len() + 1)
                > MAX_CAPTURED_SSE_EVENT_BYTES
            {
                self.discard_current_event();
                continue;
            }

            let line_bytes = self.line_buffer.len() + line.len() + 1;
            self.line_buffer.extend_from_slice(line);
            if self.line_buffer.last() == Some(&b'\r') {
                self.line_buffer.pop();
            }
            let line = std::mem::take(&mut self.line_buffer);
            self.event_bytes += line_bytes;
            self.observe_line(&line);
        }
    }

    pub fn finish(&mut self) {
        if self.discarding_event {
            return;
        }
        if !self.line_buffer.is_empty() {
            let line = std::mem::take(&mut self.line_buffer);
            self.observe_line(&line);
        }
        self.flush_event();
    }

    pub fn is_completed(&self) -> bool {
        self.completed
    }

    pub fn output_text(&self) -> &str {
        &self.output_text
    }

    pub fn is_truncated(&self) -> bool {
        self.is_truncated
    }

    fn observe_line(&mut self, line: &[u8]) {
        if line.is_empty() {
            self.flush_event();
            return;
        }
        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        if let Some(event_name) = line.strip_prefix("event:") {
            self.event_name = Some(event_name.trim().to_string());
        } else if let Some(data) = line.strip_prefix("data:") {
            self.data_lines.push(data.trim_start().to_string());
        }
    }

    fn flush_event(&mut self) {
        self.event_bytes = 0;
        if self.data_lines.is_empty() {
            self.event_name = None;
            return;
        }
        let data = std::mem::take(&mut self.data_lines).join("\n");
        let event_name = self.event_name.take();
        match self.protocol {
            RawStreamProtocol::ChatCompletions => {
                if data.trim() == "[DONE]" {
                    self.completed = true;
                    return;
                }
                if let Some(text) = serde_json::from_str::<Value>(&data).ok().and_then(|value| {
                    value
                        .pointer("/choices/0/delta/content")?
                        .as_str()
                        .map(str::to_string)
                }) {
                    self.append_text(&text);
                }
            }
            RawStreamProtocol::AnthropicMessages => {
                if event_name.as_deref() == Some("message_stop") {
                    self.completed = true;
                    return;
                }
                if event_name.as_deref() == Some("content_block_delta") {
                    if let Some(text) =
                        serde_json::from_str::<Value>(&data).ok().and_then(|value| {
                            value.pointer("/delta/text")?.as_str().map(str::to_string)
                        })
                    {
                        self.append_text(&text);
                    }
                }
            }
            RawStreamProtocol::ImageGeneration => {
                if serde_json::from_str::<Value>(&data)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("type")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .as_deref()
                    == Some("image_generation.completed")
                {
                    self.completed = true;
                }
            }
        }
    }

    fn append_text(&mut self, text: &str) {
        let remaining = MAX_CAPTURED_STREAM_TEXT_BYTES.saturating_sub(self.output_text.len());
        if remaining == 0 {
            self.is_truncated = true;
            return;
        }
        let mut end = remaining.min(text.len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        self.output_text.push_str(&text[..end]);
        self.is_truncated |= end < text.len();
    }

    fn discard_current_event(&mut self) {
        self.line_buffer.clear();
        self.event_name = None;
        self.data_lines.clear();
        self.event_bytes = 0;
        self.discarding_event = true;
        self.is_truncated = true;
    }
}

#[cfg(test)]
mod tests {
    use super::{RawStreamContentCapture, RawStreamProtocol};

    #[test]
    fn drops_an_oversized_unterminated_sse_event() {
        let mut capture = RawStreamContentCapture::new(RawStreamProtocol::ImageGeneration);
        capture.observe_chunk(&vec![b'x'; 256 * 1024]);

        assert!(capture.line_buffer.is_empty());
        assert!(capture.data_lines.is_empty());
    }
}
