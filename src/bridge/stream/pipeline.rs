use std::{collections::VecDeque, pin::Pin};

use axum::body::Bytes;
use futures::{Stream, StreamExt};

pub(crate) trait ByteStreamDecoder {
    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes>;

    fn finish(&mut self) -> Vec<Bytes>;
}

struct StreamState<D> {
    upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    decoder: D,
    pending: VecDeque<Bytes>,
    upstream_done: bool,
}

pub(crate) fn map_response_stream<D>(
    response: reqwest::Response,
    decoder: D,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    D: ByteStreamDecoder,
{
    let upstream = response.bytes_stream();
    let state = StreamState {
        upstream: Box::pin(upstream),
        decoder,
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

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use axum::{body::Body, response::Response, routing::get, Router};
    use futures::{stream, StreamExt};
    use tokio::net::TcpListener;

    use super::{map_response_stream, ByteStreamDecoder};

    #[derive(Default)]
    struct RecordingDecoder {
        finished: bool,
    }

    impl ByteStreamDecoder for RecordingDecoder {
        fn push_chunk(&mut self, chunk: &[u8]) -> Vec<axum::body::Bytes> {
            vec![
                axum::body::Bytes::from(format!("first:{}", chunk.len())),
                axum::body::Bytes::from(format!("second:{}", chunk.len())),
            ]
        }

        fn finish(&mut self) -> Vec<axum::body::Bytes> {
            self.finished = true;
            vec![axum::body::Bytes::from_static(b"finished")]
        }
    }

    #[tokio::test]
    async fn map_response_stream_preserves_pending_order_and_flushes_on_eof() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("local addr");
        let app = Router::new().route(
            "/stream",
            get(|| async {
                let body = Body::from_stream(stream::iter([
                    Ok::<_, Infallible>(axum::body::Bytes::from_static(b"ab")),
                    Ok::<_, Infallible>(axum::body::Bytes::from_static(b"cde")),
                ]));
                Response::new(body)
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::Client::new()
            .get(format!("http://{addr}/stream"))
            .send()
            .await
            .expect("test response");
        let chunks = map_response_stream(response, RecordingDecoder::default())
            .map(|chunk| chunk.expect("mapped chunk"))
            .collect::<Vec<_>>()
            .await;

        let chunk_text = chunks
            .iter()
            .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
            .collect::<Vec<_>>();

        assert_eq!(chunk_text.last(), Some(&"finished"));
        assert!(chunk_text
            .windows(2)
            .any(|window| window[0].starts_with("first:") && window[1].starts_with("second:")));
    }
}
