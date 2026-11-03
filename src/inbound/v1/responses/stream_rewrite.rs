use std::{collections::VecDeque, pin::Pin};

use axum::body::Bytes;
use futures::{stream, Stream, StreamExt};
use serde_json::Value;

pub(super) fn rewrite_response_ids<S>(
    upstream: S,
    local_id: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let state = RewriteState {
        upstream: Box::pin(upstream),
        rewriter: ResponseIdRewriter::new(local_id),
        pending: VecDeque::new(),
        upstream_done: false,
    };
    stream::unfold(state, |mut state| async move {
        loop {
            if let Some(chunk) = state.pending.pop_front() {
                return Some((Ok(chunk), state));
            }
            if state.upstream_done {
                return None;
            }
            match state.upstream.next().await {
                Some(Ok(chunk)) => state.pending.extend(state.rewriter.push_chunk(&chunk)),
                Some(Err(error)) => return Some((Err(error), state)),
                None => {
                    state.upstream_done = true;
                    state.pending.extend(state.rewriter.finish());
                }
            }
        }
    })
}

struct RewriteState {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    rewriter: ResponseIdRewriter,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
}

struct ResponseIdRewriter {
    event_buffer: Vec<u8>,
    local_id: String,
}

impl ResponseIdRewriter {
    fn new(local_id: String) -> Self {
        Self {
            event_buffer: Vec::new(),
            local_id,
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.event_buffer.extend_from_slice(chunk);
        let mut output = Vec::new();
        while let Some(end) = event_end(&self.event_buffer) {
            let event = self.event_buffer.drain(..end).collect::<Vec<_>>();
            output.push(self.rewrite_event(&event));
        }
        output
    }

    fn finish(&mut self) -> Vec<Bytes> {
        if self.event_buffer.is_empty() {
            return Vec::new();
        }
        let event = std::mem::take(&mut self.event_buffer);
        vec![self.rewrite_event(&event)]
    }

    fn rewrite_event(&self, event: &[u8]) -> Bytes {
        let lines = split_lines(event);
        let data_lines = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| data_payload(line).map(|data| (index, data)))
            .collect::<Vec<_>>();
        if data_lines.is_empty() {
            return Bytes::copy_from_slice(event);
        }
        let joined = data_lines
            .iter()
            .map(|(_, data)| *data)
            .collect::<Vec<_>>()
            .join("\n");
        let Ok(mut value) = serde_json::from_str::<Value>(&joined) else {
            return Bytes::copy_from_slice(event);
        };
        let original = value.clone();
        rewrite_value(&mut value, &self.local_id);
        if value == original {
            return Bytes::copy_from_slice(event);
        }
        let serialized =
            serde_json::to_string(&value).expect("serializing an SSE JSON value cannot fail");
        let first_data_line = data_lines[0].0;
        let data_indexes = data_lines
            .iter()
            .map(|(index, _)| *index)
            .collect::<std::collections::HashSet<_>>();
        let mut output = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if index == first_data_line {
                output.extend_from_slice(
                    format!("data: {serialized}{}", line_ending(line)).as_bytes(),
                );
            } else if !data_indexes.contains(&index) {
                output.extend_from_slice(line);
            }
        }
        Bytes::from(output)
    }
}

fn event_end(buffer: &[u8]) -> Option<usize> {
    let mut line_start = 0;
    for (index, byte) in buffer.iter().enumerate() {
        if *byte == b'\n' {
            let line = &buffer[line_start..index];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                return Some(index + 1);
            }
            line_start = index + 1;
        }
    }
    None
}

fn split_lines(event: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in event.iter().enumerate() {
        if *byte == b'\n' {
            lines.push(&event[start..=index]);
            start = index + 1;
        }
    }
    if start < event.len() {
        lines.push(&event[start..]);
    }
    lines
}

fn data_payload(line: &[u8]) -> Option<&str> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let text = std::str::from_utf8(line).ok()?;
    let data = text.strip_prefix("data:")?;
    Some(data.strip_prefix(' ').unwrap_or(data))
}

fn line_ending(line: &[u8]) -> &'static str {
    if line.ends_with(b"\r\n") {
        "\r\n"
    } else if line.ends_with(b"\n") {
        "\n"
    } else {
        ""
    }
}

fn rewrite_value(value: &mut Value, local_id: &str) {
    if let Some(response) = value.get_mut("response").and_then(Value::as_object_mut) {
        if response.get("id").is_some_and(Value::is_string) {
            response.insert("id".to_string(), Value::String(local_id.to_string()));
        }
    }
    if value.get("response_id").is_some_and(Value::is_string) {
        value["response_id"] = Value::String(local_id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use futures::{stream, TryStreamExt};

    use super::rewrite_response_ids;

    async fn rewrite(input: &'static [u8], local_id: &str) -> String {
        let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(input))]);
        let chunks = rewrite_response_ids(stream, local_id.to_string())
            .try_collect::<Vec<_>>()
            .await
            .expect("rewrite stream");
        chunks
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.to_vec()).expect("utf8 chunk"))
            .collect()
    }

    #[tokio::test]
    async fn rewrites_only_response_identifiers() {
        let output = rewrite(
            b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_upstream\"}}\n\n\
              event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"item_upstream\",\"call_id\":\"call_1\"},\"item_id\":\"item_upstream\"}\n\n\
              event: response.completed\ndata: {\"type\":\"response.completed\",\"response_id\":\"resp_upstream\",\"response\":{\"id\":\"resp_upstream\",\"output\":[{\"type\":\"reasoning\",\"id\":\"rs_1\"}]}}\n\n\
              data: [DONE]\n\n",
            "resp_local",
        )
        .await;

        assert!(output.contains(r#""response":{"id":"resp_local""#));
        assert!(output.contains(r#""response_id":"resp_local""#));
        assert!(!output.contains("resp_upstream"));
        assert!(output.contains(r#""item":{"id":"item_upstream","call_id":"call_1"}"#));
        assert!(output.contains(r#""item_id":"item_upstream""#));
        assert!(output.contains(r#""id":"rs_1""#));
        assert!(output.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn rewrites_an_event_split_across_chunks() {
        let stream = stream::iter([
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_up",
            )),
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"stream\"}}\n\ndata: [DONE]\n\n",
            )),
        ]);
        let chunks = rewrite_response_ids(stream, "resp_local".to_string())
            .try_collect::<Vec<_>>()
            .await
            .expect("rewrite stream");
        let output = chunks
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.to_vec()).expect("utf8 chunk"))
            .collect::<String>();

        assert!(output.contains(r#""id":"resp_local""#));
        assert!(!output.contains("resp_upstream"));
        assert!(output.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn flushes_a_final_line_without_a_newline() {
        let output = rewrite(
            b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_upstream\"}}",
            "resp_local",
        )
        .await;

        assert!(output.contains(r#""id":"resp_local""#));
        assert!(!output.ends_with('\n'));
    }
}
