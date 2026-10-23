use std::collections::BTreeMap;

use serde_json::Value;

pub enum RawStreamProtocol {
    ChatCompletions,
    ImageGeneration,
}

pub struct RawStreamContentCapture {
    protocol: RawStreamProtocol,
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    output_text: String,
    reasoning_text: String,
    tool_calls: BTreeMap<usize, CapturedToolCall>,
    completed: bool,
}

#[derive(Debug, Default)]
struct CapturedToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl RawStreamContentCapture {
    pub fn new(protocol: RawStreamProtocol) -> Self {
        Self {
            protocol,
            line_buffer: Vec::new(),
            data_lines: Vec::new(),
            output_text: String::new(),
            reasoning_text: String::new(),
            tool_calls: BTreeMap::new(),
            completed: false,
        }
    }

    pub fn observe_chunk(&mut self, mut chunk: &[u8]) {
        while !chunk.is_empty() {
            let Some(newline) = chunk.iter().position(|byte| *byte == b'\n') else {
                self.line_buffer.extend_from_slice(chunk);
                return;
            };
            let line = &chunk[..newline];
            chunk = &chunk[newline + 1..];

            self.line_buffer.extend_from_slice(line);
            if self.line_buffer.last() == Some(&b'\r') {
                self.line_buffer.pop();
            }
            let line = std::mem::take(&mut self.line_buffer);
            self.observe_line(&line);
        }
    }

    pub fn finish(&mut self) {
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

    pub fn reasoning_text(&self) -> &str {
        &self.reasoning_text
    }

    pub fn captured_tool_calls(&self) -> Vec<(String, String, String)> {
        self.tool_calls
            .values()
            .map(|tool_call| {
                (
                    tool_call.id.clone(),
                    tool_call.name.clone(),
                    tool_call.arguments.clone(),
                )
            })
            .collect()
    }

    fn observe_line(&mut self, line: &[u8]) {
        if line.is_empty() {
            self.flush_event();
            return;
        }
        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        if let Some(data) = line.strip_prefix("data:") {
            self.data_lines.push(data.trim_start().to_string());
        }
    }

    fn flush_event(&mut self) {
        if self.data_lines.is_empty() {
            return;
        }
        let data = std::mem::take(&mut self.data_lines).join("\n");
        let value = serde_json::from_str::<Value>(&data).ok();
        match self.protocol {
            RawStreamProtocol::ChatCompletions => {
                if data.trim() == "[DONE]" {
                    self.completed = true;
                    return;
                }
                if let Some(text) = value.as_ref().and_then(|value| {
                    value
                        .pointer("/choices/0/delta/content")?
                        .as_str()
                        .map(str::to_string)
                }) {
                    self.append_text(&text);
                }
                if let Some(reasoning) = value.as_ref().and_then(|value| {
                    value
                        .pointer("/choices/0/delta/reasoning_content")?
                        .as_str()
                }) {
                    self.reasoning_text.push_str(reasoning);
                }
                if let Some(tool_calls) = value.as_ref().and_then(|value| {
                    value
                        .pointer("/choices/0/delta/tool_calls")
                        .and_then(Value::as_array)
                }) {
                    for tool_call in tool_calls {
                        self.observe_chat_tool_call(tool_call);
                    }
                }
            }
            RawStreamProtocol::ImageGeneration => {
                if value
                    .as_ref()
                    .and_then(|value| value.get("type").and_then(Value::as_str))
                    == Some("image_generation.completed")
                {
                    self.completed = true;
                }
            }
        }
    }

    fn observe_chat_tool_call(&mut self, value: &Value) {
        let index = value
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize;
        let tool_call = self.tool_calls.entry(index).or_default();
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            tool_call.id = id.to_string();
        }
        if let Some(name) = value.pointer("/function/name").and_then(Value::as_str) {
            tool_call.name = name.to_string();
        }
        if let Some(arguments) = value.pointer("/function/arguments").and_then(Value::as_str) {
            tool_call.arguments.push_str(arguments);
        }
    }

    fn append_text(&mut self, text: &str) {
        self.output_text.push_str(text);
    }
}

#[cfg(test)]
mod tests {
    use super::{RawStreamContentCapture, RawStreamProtocol};

    #[test]
    fn accepts_an_unterminated_sse_event_larger_than_the_old_capture_limit() {
        let mut capture = RawStreamContentCapture::new(RawStreamProtocol::ImageGeneration);
        capture.observe_chunk(&vec![b'x'; 256 * 1024]);

        assert_eq!(capture.line_buffer.len(), 256 * 1024);
        assert!(capture.data_lines.is_empty());
    }

    #[test]
    fn chat_capture_keeps_tool_call_arguments() {
        let mut capture = RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions);
        capture.observe_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"path\":\"Cargo.toml\"}"}}]}}]}

data: [DONE]

"#,
        );
        capture.finish();

        assert_eq!(
            capture.captured_tool_calls(),
            vec![(
                "call_1".to_string(),
                "read_file".to_string(),
                "{\"path\":\"Cargo.toml\"}".to_string(),
            )]
        );
    }

    #[test]
    fn captures_streaming_reasoning_text() {
        let mut chat = RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions);
        chat.observe_chunk(
            b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"}}]}\n\ndata: [DONE]\n\n",
        );
        chat.finish();
        assert_eq!(chat.reasoning_text(), "think");
    }
}
