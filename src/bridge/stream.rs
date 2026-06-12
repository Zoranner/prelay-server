use std::{
    collections::{BTreeMap, VecDeque},
    pin::Pin,
};

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

pub fn responses_sse_response_to_anthropic_messages_sse(
    response: reqwest::Response,
    model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    let upstream = response.bytes_stream();
    let state = ResponsesSseAnthropicMessagesSseStreamState {
        upstream: Box::pin(upstream),
        decoder: ResponsesSseAnthropicMessagesSseDecoder::new(model),
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

pub fn anthropic_messages_sse_response_to_responses_sse(
    response: reqwest::Response,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    let upstream = response.bytes_stream();
    let state = AnthropicMessagesToResponsesSseStreamState {
        upstream: Box::pin(upstream),
        decoder: AnthropicMessagesToResponsesSseDecoder::default(),
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
    tool_calls: BTreeMap<usize, ChatToolCallState>,
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
            output.extend(self.process_line(&line));
            output.extend(self.flush_event());
        }
        if !self.completed {
            self.completed = true;
            output.extend(self.finish_response());
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
        if let Some(delta) = event.text_delta {
            output.push(responses_text_delta_sse(&delta));
        }
        for delta in event.tool_call_deltas {
            output.extend(self.tool_call_delta_to_responses_sse(delta));
        }
        if event.finished {
            self.completed = true;
            output.extend(self.finish_response());
        }
        output
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
        for (index, tool_call) in self.tool_calls.iter_mut() {
            if !tool_call.done {
                tool_call.done = true;
                output.push(responses_function_call_arguments_done_sse(
                    *index, tool_call,
                ));
                output.push(responses_output_item_done_sse(*index, tool_call));
            }
        }
        output.push(responses_completed_sse());
        output
    }
}

struct AnthropicMessagesSseStreamState {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    decoder: AnthropicMessagesSseDecoder,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
}

struct ResponsesSseAnthropicMessagesSseStreamState {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    decoder: ResponsesSseAnthropicMessagesSseDecoder,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
}

struct ResponsesSseAnthropicMessagesSseDecoder {
    line_buffer: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
    anthropic: AnthropicMessagesSseDecoder,
}

impl ResponsesSseAnthropicMessagesSseDecoder {
    fn new(model: String) -> Self {
        Self {
            line_buffer: Vec::new(),
            event_name: None,
            data_lines: Vec::new(),
            anthropic: AnthropicMessagesSseDecoder::new(model),
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
        if !self.anthropic.completed {
            output.push(self.anthropic.finish_message());
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

struct AnthropicMessagesSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    tool_calls: BTreeMap<usize, ChatToolCallState>,
    completed: bool,
    message_started: bool,
    content_block_started: bool,
    used_tool: bool,
    message_id: String,
    model: String,
}

impl AnthropicMessagesSseDecoder {
    fn new(model: String) -> Self {
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
        if self.completed {
            return Vec::new();
        }
        if data.trim() == "[DONE]" {
            return vec![self.finish_message()];
        }

        let Some(event) = decode_chat_sse_event(&data) else {
            return Vec::new();
        };

        let mut output = Vec::new();
        if let Some(delta) = event.text_delta {
            output.push(self.text_delta(&delta));
        }
        for delta in event.tool_call_deltas {
            output.extend(self.tool_call_delta(delta));
        }
        if event.finished {
            output.push(self.finish_message());
        }
        output
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
        for (index, tool_call) in self.tool_calls.iter_mut() {
            if !tool_call.done {
                tool_call.done = true;
                chunk.push_str(&anthropic_content_block_stop_at_index_sse(*index));
            }
        }
        chunk.push_str(&anthropic_message_delta_sse(if self.used_tool {
            "tool_use"
        } else {
            "end_turn"
        }));
        chunk.push_str(&anthropic_message_stop_sse());
        Bytes::from(chunk)
    }
}

struct AnthropicMessagesToResponsesSseStreamState {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    decoder: AnthropicMessagesToResponsesSseDecoder,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
}

#[derive(Default)]
struct AnthropicMessagesToResponsesSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<String>,
    completed: bool,
}

impl AnthropicMessagesToResponsesSseDecoder {
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
            self.completed = true;
            output.push(responses_completed_sse());
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

pub fn responses_text_delta_sse(delta: &str) -> Bytes {
    Bytes::from(format!(
        "event: response.output_text.delta\ndata: {delta}\n\n"
    ))
}

pub fn responses_completed_sse() -> Bytes {
    Bytes::from_static(b"event: response.completed\ndata: {}\n\ndata: [DONE]\n\n")
}

fn responses_function_call_added_sse(index: usize, tool_call: &ChatToolCallState) -> Bytes {
    responses_sse_event(
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "output_index": index,
            "item": {
                "type": "function_call",
                "id": tool_call.id,
                "call_id": tool_call.id,
                "name": tool_call.name,
                "arguments": ""
            }
        }),
    )
}

fn responses_function_call_arguments_delta_sse(
    index: usize,
    tool_call: &ChatToolCallState,
    delta: &str,
) -> Bytes {
    responses_sse_event(
        "response.function_call_arguments.delta",
        json!({
            "type": "response.function_call_arguments.delta",
            "item_id": tool_call.id,
            "output_index": index,
            "call_id": tool_call.id,
            "delta": delta
        }),
    )
}

fn responses_function_call_arguments_done_sse(
    index: usize,
    tool_call: &ChatToolCallState,
) -> Bytes {
    responses_sse_event(
        "response.function_call_arguments.done",
        json!({
            "type": "response.function_call_arguments.done",
            "item_id": tool_call.id,
            "output_index": index,
            "call_id": tool_call.id,
            "name": tool_call.name,
            "arguments": tool_call.arguments
        }),
    )
}

fn responses_output_item_done_sse(index: usize, tool_call: &ChatToolCallState) -> Bytes {
    responses_sse_event(
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": index,
            "item": {
                "type": "function_call",
                "id": tool_call.id,
                "call_id": tool_call.id,
                "name": tool_call.name,
                "arguments": tool_call.arguments
            }
        }),
    )
}

fn responses_sse_event(event: &str, data: Value) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
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

fn anthropic_message_delta_sse(stop_reason: &str) -> String {
    anthropic_sse_event(
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
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

#[derive(Default)]
struct ChatToolCallState {
    id: String,
    name: String,
    arguments: String,
    added: bool,
    done: bool,
}

struct ChatSseEvent {
    text_delta: Option<String>,
    tool_call_deltas: Vec<ChatToolCallDelta>,
    finished: bool,
}

struct ChatToolCallDelta {
    index: usize,
    id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
}

struct AnthropicMessagesSseEvent {
    text_delta: Option<String>,
    finished: bool,
}

fn decode_chat_sse_event(data: &str) -> Option<ChatSseEvent> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())?;
    let delta = choice.get("delta");
    let text_delta = delta
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
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
    let finished = choice.get("finish_reason").is_some_and(|finish_reason| {
        !finish_reason.is_null()
            && finish_reason
                .as_str()
                .is_none_or(|finish_reason| !finish_reason.is_empty())
    });

    Some(ChatSseEvent {
        text_delta,
        tool_call_deltas,
        finished,
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
    use super::{
        responses_completed_sse, responses_text_delta_sse, AnthropicMessagesSseDecoder,
        AnthropicMessagesToResponsesSseDecoder, ChatSseDecoder,
        ResponsesSseAnthropicMessagesSseDecoder,
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
    fn decodes_chat_sse_tool_call_to_responses_sse() {
        let mut decoder = ChatSseDecoder::default();

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
        assert!(output.ends_with("event: response.completed\ndata: {}\n\ndata: [DONE]\n\n"));
    }

    #[test]
    fn decodes_chat_sse_split_tool_call_arguments_to_responses_sse() {
        let mut decoder = ChatSseDecoder::default();

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
}
