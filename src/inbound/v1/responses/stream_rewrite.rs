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
    line_buffer: Vec<u8>,
    local_id: String,
}

impl ResponseIdRewriter {
    fn new(local_id: String) -> Self {
        Self {
            line_buffer: Vec::new(),
            local_id,
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes> {
        self.line_buffer.extend_from_slice(chunk);
        let mut output = Vec::new();
        while let Some(index) = self.line_buffer.iter().position(|byte| *byte == b'\n') {
            let line = self.line_buffer.drain(..=index).collect::<Vec<_>>();
            output.push(self.rewrite_line(&line));
        }
        output
    }

    fn finish(&mut self) -> Vec<Bytes> {
        if self.line_buffer.is_empty() {
            return Vec::new();
        }
        let line = std::mem::take(&mut self.line_buffer);
        vec![self.rewrite_line(&line)]
    }

    fn rewrite_line(&self, line: &[u8]) -> Bytes {
        let Ok(text) = std::str::from_utf8(line) else {
            return Bytes::copy_from_slice(line);
        };
        let (body, ending) = if let Some(body) = text.strip_suffix("\r\n") {
            (body, "\r\n")
        } else if let Some(body) = text.strip_suffix('\n') {
            (body, "\n")
        } else {
            (text, "")
        };
        let Some(data) = body.strip_prefix("data:") else {
            return Bytes::copy_from_slice(line);
        };
        let data = data.strip_prefix(' ').unwrap_or(data);
        let Ok(mut value) = serde_json::from_str::<Value>(data) else {
            return Bytes::copy_from_slice(line);
        };
        let original = value.clone();
        rewrite_value(&mut value, &self.local_id);
        if value == original {
            return Bytes::copy_from_slice(line);
        }
        let serialized =
            serde_json::to_string(&value).expect("serializing an SSE JSON value cannot fail");
        Bytes::from(format!("data: {serialized}{ending}"))
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
