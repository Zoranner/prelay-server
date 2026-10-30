use std::{
    collections::VecDeque,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::body::Bytes;
use futures::{stream, Stream, StreamExt};

use super::events::StreamStatsSnapshot;

pub type SharedStreamStats = Arc<Mutex<StreamStatsSnapshot>>;

pub(crate) trait ByteStreamDecoder {
    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Bytes>;

    fn finish(&mut self) -> Vec<Bytes>;

    fn set_stats(&mut self, _stats: SharedStreamStats) {}
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
    let (stream, _) = map_response_stream_with_stats(response, decoder);
    stream
}

pub(crate) fn map_response_stream_with_stats<D>(
    response: reqwest::Response,
    decoder: D,
) -> (
    impl Stream<Item = Result<Bytes, std::io::Error>>,
    SharedStreamStats,
)
where
    D: ByteStreamDecoder,
{
    map_response_stream_with_stats_and_head(response, None, decoder)
}

/// SSE 保活注释：上游静默时向下游发送，避免反向代理或客户端
/// 因长时间无数据而断开连接。
const SSE_KEEPALIVE: &[u8] = b": keep-alive\n\n";

/// 默认保活间隔。
pub(crate) const SSE_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// 给发给客户端的 SSE 字节流加保活注释：静默超过 `interval` 时插入一
/// 条注释行，注释不经过记录与统计包装。
pub(crate) fn with_sse_keepalive_every<S, E>(
    stream: S,
    interval: Duration,
) -> impl Stream<Item = Result<Bytes, E>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: Send + 'static,
{
    stream::unfold(Box::pin(stream), move |mut stream| async move {
        match tokio::time::timeout(interval, stream.next()).await {
            // 上游有数据（包括错误块）：原样透传。
            Ok(Some(item)) => Some((item, stream)),
            // 上游正常结束。
            Ok(None) => None,
            // 静默超过间隔：插入保活注释，继续等待上游。
            Err(_) => Some((Ok(Bytes::from_static(SSE_KEEPALIVE)), stream)),
        }
    })
}

/// 使用默认间隔的 `with_sse_keepalive_every`。
pub(crate) fn with_sse_keepalive<S, E>(stream: S) -> impl Stream<Item = Result<Bytes, E>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: Send + 'static,
{
    with_sse_keepalive_every(stream, SSE_KEEPALIVE_INTERVAL)
}

/// 与 `map_response_stream_with_stats` 相同，但把已经检出的首块拼回上游字节流，
/// 避免校验消费掉的数据丢失。
pub(crate) fn map_response_stream_with_stats_and_head<D>(
    response: reqwest::Response,
    head: Option<Bytes>,
    mut decoder: D,
) -> (
    impl Stream<Item = Result<Bytes, std::io::Error>>,
    SharedStreamStats,
)
where
    D: ByteStreamDecoder,
{
    let upstream = upstream_bytes_stream(response, head);
    let stats = Arc::new(Mutex::new(StreamStatsSnapshot::default()));
    decoder.set_stats(stats.clone());
    let state = StreamState {
        upstream,
        decoder,
        pending: VecDeque::new(),
        upstream_done: false,
    };

    (
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
        }),
        stats,
    )
}

/// 上游字节流；`head` 是校验阶段取出的首块，拼回后再交给后续处理。
pub(crate) fn upstream_bytes_stream(
    response: reqwest::Response,
    head: Option<Bytes>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>> {
    let upstream = response.bytes_stream();
    match head {
        Some(bytes) => Box::pin(
            stream::once(async move { Ok::<Bytes, reqwest::Error>(bytes) }).chain(upstream),
        ),
        None => Box::pin(upstream),
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::time::Duration;

    use axum::{body::Body, response::Response, routing::get, Router};
    use futures::{channel::mpsc, stream, StreamExt};
    use tokio::net::TcpListener;

    use super::{map_response_stream, with_sse_keepalive_every, ByteStreamDecoder, SSE_KEEPALIVE};

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

    #[tokio::test]
    async fn keepalive_is_emitted_while_upstream_is_silent() {
        let (mut tx, rx) = mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(4);
        let mut stream = Box::pin(with_sse_keepalive_every(rx, Duration::from_millis(30)));

        let first = stream.next().await.expect("keepalive item");
        assert_eq!(
            first.expect("keepalive chunk").as_ref(),
            SSE_KEEPALIVE,
            "a silent upstream is kept alive with an SSE comment"
        );

        tx.try_send(Ok(axum::body::Bytes::from_static(b"data: x\n\n")))
            .expect("send upstream chunk");
        let second = stream.next().await.expect("upstream item");
        assert_eq!(second.expect("upstream chunk").as_ref(), b"data: x\n\n");

        drop(tx);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn flowing_stream_passes_items_untouched() {
        let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = vec![
            Ok(axum::body::Bytes::from_static(b"data: a\n\n")),
            Ok(axum::body::Bytes::from_static(b"data: b\n\n")),
        ];
        let stream = with_sse_keepalive_every(stream::iter(chunks), Duration::from_secs(15));
        let collected: Vec<_> = stream.collect().await;

        assert_eq!(collected.len(), 2, "no keepalive is added while data flows");
        assert_eq!(
            collected[0].as_ref().expect("chunk").as_ref(),
            b"data: a\n\n"
        );
        assert_eq!(
            collected[1].as_ref().expect("chunk").as_ref(),
            b"data: b\n\n"
        );
    }
}
