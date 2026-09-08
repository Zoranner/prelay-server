use std::{
    pin::Pin,
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use tokio::sync::mpsc;

use crate::{
    activity::{activity_content_from_text_with_media_or_empty, policy, RawStreamContentCapture},
    bridge::stream::{SharedStreamStats, StreamStatsSnapshot},
    stats::{ActivityInsert, StreamActivityUpdate},
    storage::{Storage, StorageError},
};

use super::persistence::{
    log_stream_storage_failure, start_stream_record_with_id, update_stream_log,
};

pub(super) type RecordedStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;
const DOWNSTREAM_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const RECORDED_STREAM_CHANNEL_CAPACITY: usize = 16;

#[derive(Clone, Copy)]
pub(super) enum RecordingMode {
    Required,
    BestEffort,
}

pub(super) struct StreamRecordOptions {
    pub(super) stats: Option<SharedStreamStats>,
    pub(super) input_text: String,
    pub(super) content_capture: Option<RawStreamContentCapture>,
    pub(super) log_id: String,
    pub(super) mode: RecordingMode,
}

impl StreamRecordOptions {
    pub(super) fn required(
        stats: Option<SharedStreamStats>,
        input_text: String,
        content_capture: Option<RawStreamContentCapture>,
    ) -> Self {
        Self {
            stats,
            input_text,
            content_capture,
            log_id: String::new(),
            mode: RecordingMode::Required,
        }
    }

    pub(super) fn best_effort(
        stats: Option<SharedStreamStats>,
        input_text: String,
        content_capture: Option<RawStreamContentCapture>,
    ) -> Self {
        Self {
            stats,
            input_text,
            content_capture,
            log_id: String::new(),
            mode: RecordingMode::BestEffort,
        }
    }
}

pub(super) async fn prepare_stream_with_log_id<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    options: StreamRecordOptions,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let mut upstream = Box::pin(stream);
    let first = upstream.next().await;
    let mut state = StreamRecordState {
        storage,
        identity_id,
        stream: upstream,
        log: Some(log),
        log_id: options.log_id,
        started_at,
        stats: options.stats,
        input_text: options.input_text,
        content_capture: options.content_capture,
        mode: options.mode,
        upstream_request_id: None,
        inserted: false,
        failed: false,
        usage_recorded: false,
        content_finalized: false,
    };

    let Some(first) = first else {
        state.record_empty_stream().await?;
        return Ok(Box::pin(futures::stream::empty()));
    };
    state.start(&first).await?;
    state.observe_item(&first).await;

    let (sender, receiver) = mpsc::channel(RECORDED_STREAM_CHANNEL_CAPACITY);
    tokio::spawn(forward_recorded_stream(state, first, sender));
    Ok(Box::pin(futures::stream::unfold(
        receiver,
        |mut receiver| async move { receiver.recv().await.map(|item| (item, receiver)) },
    )))
}

async fn forward_recorded_stream(
    mut state: StreamRecordState,
    first: Result<Bytes, std::io::Error>,
    sender: mpsc::Sender<Result<Bytes, std::io::Error>>,
) {
    let first_failed = first.is_err();
    let mut sender = Some(sender);
    let mut drain_deadline = None;
    if let Some(current_sender) = sender.as_ref() {
        if current_sender.send(first).await.is_err() {
            sender = None;
            drain_deadline = Some(tokio::time::Instant::now() + DOWNSTREAM_DRAIN_TIMEOUT);
        }
    }
    if first_failed {
        return;
    }

    loop {
        let next = match drain_deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, state.stream.next()).await {
                Ok(item) => item,
                Err(_) => {
                    state.record_downstream_disconnect().await;
                    return;
                }
            },
            None => state.stream.next().await,
        };
        let Some(item) = next else {
            if drain_deadline.is_some() {
                state.record_downstream_disconnect().await;
            } else {
                state.record_stream_end().await;
            }
            return;
        };
        let item_failed = item.is_err();
        state.observe_item(&item).await;
        if let Some(current_sender) = sender.as_ref() {
            if current_sender.send(item).await.is_err() {
                sender = None;
                drain_deadline = Some(tokio::time::Instant::now() + DOWNSTREAM_DRAIN_TIMEOUT);
            }
        }
        if item_failed {
            return;
        }
    }
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
    mode: RecordingMode,
    upstream_request_id: Option<String>,
    inserted: bool,
    failed: bool,
    usage_recorded: bool,
    content_finalized: bool,
}

impl StreamRecordState {
    async fn start(&mut self, item: &Result<Bytes, std::io::Error>) -> Result<(), StorageError> {
        let Some(mut log) = self.log.take() else {
            return Ok(());
        };
        let first_token_ms = self.started_at.elapsed().as_millis() as i64;
        log.latency_ms = first_token_ms;
        match item {
            Ok(_) => {
                log.status = "streaming".to_string();
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
        let content = activity_content_from_text_with_media_or_empty(
            &self.input_text,
            "",
            None,
            policy().max_bytes,
        );
        match start_stream_record_with_id(
            &self.storage,
            &self.identity_id,
            &self.log_id,
            log,
            content,
        )
        .await
        {
            Ok(()) => {
                self.inserted = true;
                Ok(())
            }
            Err(error) => match self.mode {
                RecordingMode::Required => Err(error),
                RecordingMode::BestEffort => {
                    self.failed = true;
                    Ok(())
                }
            },
        }
    }

    async fn observe_item(&mut self, item: &Result<Bytes, std::io::Error>) {
        if self.inserted {
            if let Err(error) = item {
                if self.failed {
                    self.finalize_content().await;
                } else {
                    self.record_stream_error(error.to_string()).await;
                }
            }
        }
        if let (Ok(chunk), Some(content_capture)) = (item, &mut self.content_capture) {
            content_capture.observe_chunk(chunk);
        }
        if self.inserted && !self.failed {
            self.record_final_usage().await;
        }
    }

    async fn record_stream_end(&mut self) {
        if !self.inserted {
            return;
        }
        if !self.failed {
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
                latency_ms: self.started_at.elapsed().as_millis() as i64,
                tool_call_count: Some(snapshot.tool_call_count),
                upstream_request_id: self.upstream_request_id.clone(),
            };
            update_stream_log(&self.storage, &self.identity_id, &self.log_id, update).await;
        }
        self.finalize_content().await;
    }

    async fn record_final_usage(&mut self) {
        let snapshot = self.stats_snapshot();
        if self.usage_recorded || !snapshot.final_usage_seen {
            return;
        }
        self.usage_recorded = true;
        let update = StreamActivityUpdate {
            status: String::new(),
            http_status: 200,
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: None,
            cache_read_tokens: snapshot.cache_read_tokens,
            cache_write_tokens: snapshot.cache_write_tokens,
            latency_ms: self.started_at.elapsed().as_millis() as i64,
            tool_call_count: Some(snapshot.tool_call_count),
            upstream_request_id: self.upstream_request_id.clone(),
            ..Default::default()
        };
        update_stream_log(&self.storage, &self.identity_id, &self.log_id, update).await;
    }

    async fn record_empty_stream(&mut self) -> Result<(), StorageError> {
        let Some(mut log) = self.log.take() else {
            return Ok(());
        };
        log.status = "failed".to_string();
        log.http_status = 502;
        log.latency_ms = self.started_at.elapsed().as_millis() as i64;
        log.error_code = Some("empty_stream".to_string());
        log.error_message = Some("upstream stream finished without chunks".to_string());
        let content = activity_content_from_text_with_media_or_empty(
            &self.input_text,
            "",
            None,
            policy().max_bytes,
        );
        match self
            .storage
            .record_completed_activity_with_id(&self.identity_id, self.log_id.clone(), log, content)
            .await
        {
            Ok(()) => {
                self.inserted = true;
                self.failed = true;
                self.content_finalized = true;
                Ok(())
            }
            Err(error) => match self.mode {
                RecordingMode::Required => Err(error),
                RecordingMode::BestEffort => Ok(()),
            },
        }
    }

    async fn record_stream_error(&mut self, message: String) {
        self.failed = true;
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
            latency_ms: self.started_at.elapsed().as_millis() as i64,
            tool_call_count: Some(snapshot.tool_call_count),
            upstream_request_id: self.upstream_request_id.clone(),
        };
        update_stream_log(&self.storage, &self.identity_id, &self.log_id, update).await;
        self.finalize_content().await;
    }

    async fn record_downstream_disconnect(&mut self) {
        if !self.failed {
            self.failed = true;
            let snapshot = self.stats_snapshot();
            let update = StreamActivityUpdate {
                status: "failed".to_string(),
                http_status: 499,
                error_code: Some("downstream_disconnect".to_string()),
                error_message: Some("downstream client disconnected".to_string()),
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
        self.finalize_content().await;
    }

    async fn finalize_content(&mut self) {
        if self.content_finalized {
            return;
        }
        let snapshot = self.stats_snapshot();
        let (output_text, capture_truncated) =
            if let Some(content_capture) = self.content_capture.as_mut() {
                content_capture.finish();
                (
                    content_capture.output_text().to_string(),
                    content_capture.is_truncated() || !content_capture.is_completed(),
                )
            } else {
                (snapshot.output_text, !snapshot.completed)
            };
        let mut content = activity_content_from_text_with_media_or_empty(
            &self.input_text,
            &output_text,
            None,
            policy().max_bytes,
        );
        content.is_truncated |= capture_truncated;
        match self
            .storage
            .complete_stream_activity_content(content.into_draft(self.log_id.clone()))
            .await
        {
            Ok(()) => self.content_finalized = true,
            Err(error) => log_stream_storage_failure("complete-content", &error),
        }
    }

    fn stats_snapshot(&self) -> StreamStatsSnapshot {
        self.stats
            .as_ref()
            .and_then(|stats| stats.lock().ok().map(|stats| stats.clone()))
            .unwrap_or_default()
    }
}
