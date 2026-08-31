use std::{pin::Pin, time::Instant};

use bytes::Bytes;
use futures::{Stream, StreamExt};

use crate::{
    activity::{enqueue_activity_content_with_capture_best_effort, RawStreamContentCapture},
    bridge::stream::{SharedStreamStats, StreamStatsSnapshot},
    stats::{ActivityInsert, StreamActivityUpdate},
    storage::Storage,
};

use super::persistence::{insert_stream_log_with_id, update_stream_log};

pub(super) struct StreamRecordOptions {
    pub(super) stats: Option<SharedStreamStats>,
    pub(super) input_text: String,
    pub(super) content_capture: Option<RawStreamContentCapture>,
    pub(super) log_id: String,
}

pub(super) fn record_stream_with_log_id<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    options: StreamRecordOptions,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let upstream_request_id = log.upstream_request_id.clone();
    let state = StreamRecordState {
        storage,
        identity_id,
        stream: Box::pin(stream),
        log: Some(log),
        log_id: options.log_id,
        started_at,
        stats: options.stats,
        input_text: options.input_text,
        content_capture: options.content_capture,
        upstream_request_id,
        inserted: false,
        failed: false,
        usage_recorded: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        let Some(item) = state.stream.next().await else {
            state.record_stream_end().await;
            return None;
        };

        if !state.inserted {
            state.insert_first_chunk_log(&item).await;
        } else if let Err(error) = &item {
            state.record_stream_error(error.to_string()).await;
        }
        if let (Ok(chunk), Some(content_capture)) = (&item, &mut state.content_capture) {
            content_capture.observe_chunk(chunk);
        }
        if state.inserted && !state.failed {
            state.record_final_usage().await;
        }

        Some((item, state))
    })
}

struct StreamRecordState {
    storage: Storage,
    identity_id: String,
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    log: Option<ActivityInsert>,
    log_id: String,
    started_at: Instant,
    stats: Option<SharedStreamStats>,
    input_text: String,
    content_capture: Option<RawStreamContentCapture>,
    upstream_request_id: Option<String>,
    inserted: bool,
    failed: bool,
    usage_recorded: bool,
}

impl StreamRecordState {
    async fn insert_first_chunk_log(&mut self, item: &Result<Bytes, std::io::Error>) {
        let Some(mut log) = self.log.take() else {
            return;
        };
        let first_token_ms = self.started_at.elapsed().as_millis() as i64;
        log.latency_ms = first_token_ms;
        match item {
            Ok(_) => {
                log.first_token_ms = Some(first_token_ms);
            }
            Err(error) => {
                self.failed = true;
                log.status = "failed".to_string();
                log.http_status = 502;
                log.error_code = Some("stream_error".to_string());
                log.error_message = Some(error.to_string());
                log.first_token_ms = None;
            }
        }
        self.inserted =
            insert_stream_log_with_id(&self.storage, &self.identity_id, &self.log_id, log)
                .await
                .is_ok();
    }

    async fn record_stream_end(&mut self) {
        let latency_ms = self.started_at.elapsed().as_millis() as i64;
        if !self.inserted {
            self.record_empty_stream(latency_ms).await;
            return;
        }
        if self.failed {
            return;
        }

        let snapshot = self.stats_snapshot();
        let update = StreamActivityUpdate {
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: None,
            cache_read_tokens: snapshot.cache_read_tokens,
            cache_write_tokens: snapshot.cache_write_tokens,
            latency_ms,
            tool_call_count: Some(snapshot.tool_call_count),
            upstream_request_id: self.upstream_request_id.clone(),
        };
        update_stream_log(&self.storage, &self.identity_id, &self.log_id, update).await;
        let output_content = if let Some(content_capture) = self.content_capture.as_mut() {
            content_capture.finish();
            content_capture.is_completed().then(|| {
                (
                    content_capture.output_text().to_string(),
                    content_capture.is_truncated(),
                )
            })
        } else {
            snapshot
                .completed
                .then(|| (snapshot.output_text.clone(), false))
        };
        if let Some((output_text, capture_truncated)) = output_content {
            enqueue_activity_content_with_capture_best_effort(
                &self.storage,
                self.log_id.clone(),
                &self.input_text,
                &output_text,
                None,
                capture_truncated,
            )
            .await;
        }
    }

    async fn record_final_usage(&mut self) {
        let snapshot = self.stats_snapshot();
        if self.usage_recorded || !snapshot.final_usage_seen {
            return;
        }

        self.usage_recorded = true;
        let update = StreamActivityUpdate {
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: None,
            cache_read_tokens: snapshot.cache_read_tokens,
            cache_write_tokens: snapshot.cache_write_tokens,
            latency_ms: self.started_at.elapsed().as_millis() as i64,
            tool_call_count: Some(snapshot.tool_call_count),
            upstream_request_id: self.upstream_request_id.clone(),
        };
        update_stream_log(&self.storage, &self.identity_id, &self.log_id, update).await;
    }

    async fn record_empty_stream(&mut self, latency_ms: i64) {
        let Some(mut log) = self.log.take() else {
            return;
        };
        log.status = "failed".to_string();
        log.http_status = 502;
        log.latency_ms = latency_ms;
        log.error_code = Some("empty_stream".to_string());
        log.error_message = Some("upstream stream finished without chunks".to_string());
        if insert_stream_log_with_id(&self.storage, &self.identity_id, &self.log_id, log)
            .await
            .is_ok()
        {
            self.inserted = true;
            self.failed = true;
        }
    }

    async fn record_stream_error(&mut self, message: String) {
        self.failed = true;
        let latency_ms = self.started_at.elapsed().as_millis() as i64;
        let snapshot = self.stats_snapshot();
        let update = StreamActivityUpdate {
            status: "failed".to_string(),
            http_status: 502,
            error_code: Some("stream_error".to_string()),
            error_message: Some(message),
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: None,
            cache_read_tokens: snapshot.cache_read_tokens,
            cache_write_tokens: snapshot.cache_write_tokens,
            latency_ms,
            tool_call_count: Some(snapshot.tool_call_count),
            upstream_request_id: self.upstream_request_id.clone(),
        };
        update_stream_log(&self.storage, &self.identity_id, &self.log_id, update).await;
    }

    fn stats_snapshot(&self) -> StreamStatsSnapshot {
        self.stats
            .as_ref()
            .and_then(|stats| stats.lock().ok().map(|stats| stats.clone()))
            .unwrap_or_default()
    }
}
