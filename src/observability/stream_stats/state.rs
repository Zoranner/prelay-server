use std::{pin::Pin, time::Instant};

use bytes::Bytes;
use futures::Stream;

use crate::{
    activity::{
        events::{events_from_stream_capture, events_from_stream_snapshot},
        ActivityStore, ErrorEvent, RawStreamContentCapture, RolloutEventBody, RolloutStatus,
        RolloutWriter,
    },
    bridge::stream::{SharedStreamStats, StreamStatsSnapshot},
    stats::{ActivityInsert, StreamActivityUpdate},
    storage::StorageError,
};

use super::{
    options::{RecordingMode, StreamSession},
    persistence::{log_stream_storage_failure, update_stream_log},
};

mod forwarding;
mod session;

pub(super) use forwarding::{prepare_stream_with_log_id, RecordedStream};
use session::{response_from_stream_session, save_response_session};

struct StreamRecordState {
    activity_store: ActivityStore,
    identity_id: String,
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    log: Option<ActivityInsert>,
    log_id: String,
    started_at: Instant,
    stats: Option<SharedStreamStats>,
    content_capture: Option<RawStreamContentCapture>,
    rollout: Option<RolloutWriter>,
    session: Option<StreamSession>,
    mode: RecordingMode,
    upstream_request_id: Option<String>,
    inserted: bool,
    failed: bool,
    usage_recorded: bool,
    rollout_finalized: bool,
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
        let Some(rollout) = self.rollout.as_ref() else {
            return Err(StorageError::Rollout(
                "stream recording has no rollout writer".to_string(),
            ));
        };
        let result = self
            .activity_store
            .start_stream(&self.identity_id, &self.log_id, log, rollout.summary())
            .await;
        match result {
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
                    self.finalize_rollout(
                        RolloutStatus::Failed,
                        Some(502),
                        Some("stream_error".to_string()),
                        Some(error.to_string()),
                    )
                    .await;
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
            self.save_session_if_ready().await;
        }
    }

    async fn record_stream_end(&mut self) {
        if !self.inserted {
            return;
        }
        if self.failed {
            self.finalize_rollout(
                RolloutStatus::Failed,
                Some(502),
                Some("stream_error".to_string()),
                Some("upstream stream failed".to_string()),
            )
            .await;
            return;
        }
        if !self.stream_completed() {
            self.record_stream_error("upstream stream ended before a completion event".to_string())
                .await;
            return;
        }
        self.save_session_if_ready().await;
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
        update_stream_log(
            &self.activity_store,
            &self.identity_id,
            &self.log_id,
            update,
        )
        .await;
        self.finalize_rollout(RolloutStatus::Completed, Some(200), None, None)
            .await;
    }

    fn stream_completed(&self) -> bool {
        self.content_capture
            .as_ref()
            .map(RawStreamContentCapture::is_completed)
            .unwrap_or_else(|| self.stats_snapshot().completed)
    }

    async fn save_session_if_ready(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if !session.store {
            self.session = None;
            return;
        }
        let snapshot = self.stats_snapshot();
        if !snapshot.completed {
            return;
        }
        let Some(response) = response_from_stream_session(session, &snapshot) else {
            return;
        };
        let session = self.session.take().expect("session checked above");
        save_response_session(&self.activity_store, &self.identity_id, session, response).await;
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
        update_stream_log(
            &self.activity_store,
            &self.identity_id,
            &self.log_id,
            update,
        )
        .await;
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
        let Some(mut rollout) = self.rollout.take() else {
            return match self.mode {
                RecordingMode::Required => Err(StorageError::Rollout(
                    "empty stream recording has no rollout writer".to_string(),
                )),
                RecordingMode::BestEffort => Ok(()),
            };
        };
        if let Err(error) = rollout.append(RolloutEventBody::Error(ErrorEvent {
            code: Some("empty_stream".to_string()),
            message: "upstream stream finished without chunks".to_string(),
            http_status: Some(502),
        })) {
            log_stream_storage_failure(
                "append-empty-rollout",
                &StorageError::Rollout(error.to_string()),
            );
        }
        let summary = match rollout.finish_with(
            RolloutStatus::Failed,
            Some(502),
            Some("empty_stream".to_string()),
            Some("upstream stream finished without chunks".to_string()),
        ) {
            Ok(summary) => summary,
            Err(error) => {
                let error = StorageError::Rollout(error.to_string());
                return match self.mode {
                    RecordingMode::Required => Err(error),
                    RecordingMode::BestEffort => Ok(()),
                };
            }
        };
        let result = self
            .activity_store
            .insert_stream_result(&self.identity_id, self.log_id.clone(), log, summary)
            .await;
        match result {
            Ok(()) => {
                self.inserted = true;
                self.failed = true;
                self.rollout_finalized = true;
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
        self.session = None;
        let snapshot = self.stats_snapshot();
        let update = StreamActivityUpdate {
            status: "failed".to_string(),
            http_status: 502,
            error_code: Some("stream_error".to_string()),
            error_message: Some(message.clone()),
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: None,
            cache_read_tokens: snapshot.cache_read_tokens,
            cache_write_tokens: snapshot.cache_write_tokens,
            latency_ms: self.started_at.elapsed().as_millis() as i64,
            tool_call_count: Some(snapshot.tool_call_count),
            upstream_request_id: self.upstream_request_id.clone(),
        };
        update_stream_log(
            &self.activity_store,
            &self.identity_id,
            &self.log_id,
            update,
        )
        .await;
        self.finalize_rollout(
            RolloutStatus::Failed,
            Some(502),
            Some("stream_error".to_string()),
            Some(message),
        )
        .await;
    }

    async fn record_downstream_disconnect(&mut self) {
        if !self.failed {
            self.failed = true;
            self.session = None;
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
            update_stream_log(
                &self.activity_store,
                &self.identity_id,
                &self.log_id,
                update,
            )
            .await;
        }
        self.finalize_rollout(
            RolloutStatus::Failed,
            Some(499),
            Some("downstream_disconnect".to_string()),
            Some("downstream client disconnected".to_string()),
        )
        .await;
    }

    async fn finalize_rollout(
        &mut self,
        status: RolloutStatus,
        http_status: Option<u16>,
        error_code: Option<String>,
        error_message: Option<String>,
    ) {
        if self.rollout_finalized {
            return;
        }
        let snapshot = self.stats_snapshot();
        let Some(mut rollout) = self.rollout.take() else {
            return;
        };
        let mut events = if let Some(content_capture) = self.content_capture.as_mut() {
            content_capture.finish();
            events_from_stream_capture(content_capture)
        } else {
            events_from_stream_snapshot(&snapshot)
        };
        if let Some(message) = error_message.as_deref() {
            events.push(RolloutEventBody::Error(ErrorEvent {
                code: error_code.clone(),
                message: message.to_string(),
                http_status,
            }));
        }
        for event in events {
            if let Err(error) = rollout.append(event) {
                log_stream_storage_failure(
                    "append-rollout",
                    &StorageError::Rollout(error.to_string()),
                );
            }
        }
        match rollout.finish_with(status, http_status, error_code, error_message) {
            Ok(summary) => {
                if self.inserted {
                    if let Err(error) = self
                        .activity_store
                        .complete_stream(&self.log_id, summary)
                        .await
                    {
                        log_stream_storage_failure("complete-rollout", &error);
                    }
                }
            }
            Err(error) => log_stream_storage_failure(
                "finish-rollout",
                &StorageError::Rollout(error.to_string()),
            ),
        }
        self.rollout_finalized = true;
    }

    fn stats_snapshot(&self) -> StreamStatsSnapshot {
        self.stats
            .as_ref()
            .and_then(|stats| stats.lock().ok().map(|stats| stats.clone()))
            .unwrap_or_default()
    }
}
