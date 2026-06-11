use std::{pin::Pin, time::Instant};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use sqlx::SqlitePool;

use crate::stats::{insert_request_log, RequestLogInsert};

pub fn record_first_chunk<S>(
    db: SqlitePool,
    stream: S,
    log: RequestLogInsert,
    started_at: Instant,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let state = FirstChunkState {
        db,
        stream: Box::pin(stream),
        log: Some(log),
        started_at,
    };

    futures::stream::unfold(state, |mut state| async move {
        let Some(item) = state.stream.next().await else {
            state.record_empty_stream().await;
            return None;
        };

        if let Some(mut log) = state.log.take() {
            let first_token_ms = state.started_at.elapsed().as_millis() as i64;
            log.latency_ms = first_token_ms;
            match &item {
                Ok(_) => {
                    log.first_token_ms = Some(first_token_ms);
                }
                Err(error) => {
                    log.status = "failed".to_string();
                    log.http_status = 502;
                    log.error_code = Some("stream_error".to_string());
                    log.error_message = Some(error.to_string());
                    log.first_token_ms = None;
                }
            }
            insert_stream_log(&state.db, log).await;
        }

        Some((item, state))
    })
}

struct FirstChunkState {
    db: SqlitePool,
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    log: Option<RequestLogInsert>,
    started_at: Instant,
}

impl FirstChunkState {
    async fn record_empty_stream(&mut self) {
        let Some(mut log) = self.log.take() else {
            return;
        };
        log.latency_ms = self.started_at.elapsed().as_millis() as i64;
        log.error_code = Some("empty_stream".to_string());
        log.error_message = Some("upstream stream finished without chunks".to_string());
        insert_stream_log(&self.db, log).await;
    }
}

async fn insert_stream_log(db: &SqlitePool, log: RequestLogInsert) {
    if let Err(error) = insert_request_log(db, log).await {
        tracing::error!("failed to write streaming request log: {error:?}");
    }
}
