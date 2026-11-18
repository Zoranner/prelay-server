use std::{
    io,
    pin::Pin,
    time::{Duration, Instant},
};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{
    codec::{
        activity::{
            conversation_events_from_stream_capture, conversation_events_from_stream_snapshot,
        },
        bridge::stream::{SharedStreamStats, StreamStatsSnapshot},
    },
    domain::{
        activity::rollout::SessionContext,
        activity::{ContentDraft, ContentEventBody, ConversationError, RawStreamContentCapture},
        stats::StreamActivityUpdate,
    },
    service::recording::{
        options::StreamSession, state::response_from_stream_session, RecordingRuntime,
    },
    store::{
        recording_journal::{
            RecordEnvelope, RecordKind, RecordedContentEvent, ResponseStateRecord,
            TurnFinishedRecord, TurnTiming,
        },
        storage::{ResponseStateInsert, StorageError},
    },
};

pub(super) const CHANNEL_CAPACITY: usize = 16;
const DOWNSTREAM_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) struct JournalStreamState {
    pub(super) runtime: RecordingRuntime,
    pub(super) identity_id: String,
    pub(super) activity_id: String,
    pub(super) stream: Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>>,
    pub(super) started_at: Instant,
    pub(super) started_at_wall: DateTime<Utc>,
    pub(super) first_token_at: Option<String>,
    pub(super) upstream_request_id: Option<String>,
    pub(super) stats: Option<SharedStreamStats>,
    pub(super) content_capture: Option<RawStreamContentCapture>,
    pub(super) response_session: Option<StreamSession>,
    pub(super) session_context: SessionContext,
    pub(super) failed: bool,
    pub(super) failure: Option<(u16, String, String)>,
    pub(super) finished: bool,
}

impl JournalStreamState {
    fn observe_item(&mut self, item: &Result<Bytes, io::Error>) {
        match item {
            Ok(chunk) => {
                if let Some(capture) = &mut self.content_capture {
                    capture.observe_chunk(chunk);
                }
            }
            Err(error) => {
                if !self.failed {
                    self.failed = true;
                    self.failure = Some((502, "stream_error".to_string(), error.to_string()));
                }
            }
        }
    }

    fn stream_completed(&self) -> bool {
        self.content_capture
            .as_ref()
            .map(RawStreamContentCapture::is_completed)
            .unwrap_or_else(|| self.stats_snapshot().completed)
    }

    fn stats_snapshot(&self) -> StreamStatsSnapshot {
        self.stats
            .as_ref()
            .and_then(|stats| stats.lock().ok().map(|stats| stats.clone()))
            .unwrap_or_default()
    }

    async fn finish_success(&mut self) -> Result<(), StorageError> {
        if self.finished {
            return Ok(());
        }
        let snapshot = self.stats_snapshot();
        if self.failed || !snapshot.completed {
            return self
                .finish_failed(
                    502,
                    "stream_error",
                    "upstream stream ended before a completion event",
                )
                .await;
        }
        let response_state = self.response_state(&snapshot);
        if self.response_session.is_some() && response_state.is_none() {
            return self
                .finish_failed(
                    502,
                    "response_state_unavailable",
                    "stream response state could not be built",
                )
                .await;
        }
        let response_events = self.response_events(None);
        let activity = stream_update(
            "success",
            200,
            None,
            None,
            &snapshot,
            self.started_at,
            self.upstream_request_id.clone(),
        );
        self.commit_finished(activity, response_events, response_state)
            .await
    }

    async fn finish_failed(
        &mut self,
        http_status: u16,
        error_code: &str,
        error_message: &str,
    ) -> Result<(), StorageError> {
        if self.finished {
            return Ok(());
        }
        self.failed = true;
        let snapshot = self.stats_snapshot();
        let response_events = self.response_events(Some(ConversationError {
            code: Some(error_code.to_string()),
            message: error_message.to_string(),
            http_status: Some(http_status),
        }));
        let activity = stream_update(
            "failed",
            http_status,
            Some(error_code.to_string()),
            Some(error_message.to_string()),
            &snapshot,
            self.started_at,
            self.upstream_request_id.clone(),
        );
        self.commit_finished(activity, response_events, None).await
    }

    fn response_events(&mut self, error: Option<ConversationError>) -> Vec<RecordedContentEvent> {
        let finished_at = Utc::now().to_rfc3339();
        let mut events = if let Some(capture) = self.content_capture.as_mut() {
            capture.finish();
            conversation_events_from_stream_capture(capture)
        } else {
            conversation_events_from_stream_snapshot(&self.stats_snapshot())
        };
        if let Some(error) = error {
            events.push(ContentDraft::new(ContentEventBody::Error(error)));
        }
        timed_events(events, &finished_at)
    }

    fn response_state(&self, snapshot: &StreamStatsSnapshot) -> Option<ResponseStateRecord> {
        let session = self.response_session.as_ref()?;
        if snapshot.response_output_items.is_empty() {
            tracing::warn!(
                response_id = %session.response_id,
                "stream completed without exact output items; response state will not be persisted"
            );
            return None;
        }
        let response = response_from_stream_session(session, snapshot)?;
        let request_items = Value::Array(session.request_items.clone());
        let response_items = Value::Array(snapshot.response_output_items.clone());
        Some(ResponseStateRecord::from_insert(ResponseStateInsert {
            identity_id: &self.identity_id,
            response_id: &session.response_id,
            previous_response_id: session.previous_response_id.as_deref(),
            conversation_id: &session.conversation_id,
            provider_id: &session.provider_id,
            model: &session.model,
            upstream_response_id: snapshot.upstream_response_id.as_deref(),
            request_items: &request_items,
            response_items: &response_items,
            input_messages: &session.input_messages,
            response: &response,
        }))
    }

    async fn commit_finished(
        &mut self,
        activity: StreamActivityUpdate,
        response_events: Vec<RecordedContentEvent>,
        response_state: Option<ResponseStateRecord>,
    ) -> Result<(), StorageError> {
        self.finished = true;
        let finished_at = Utc::now().to_rfc3339();
        let payload = TurnFinishedRecord {
            activity,
            session: self.session_context.clone(),
            response_events,
            response_state,
            timing: TurnTiming {
                started_at: self.started_at_wall.to_rfc3339(),
                first_token_at: self.first_token_at.clone(),
                finished_at: Some(finished_at.clone()),
            },
        };
        let envelope = RecordEnvelope::from_payload(
            self.activity_id.clone(),
            self.identity_id.clone(),
            RecordKind::TurnFinished,
            finished_at,
            &payload,
        )
        .map_err(|error| StorageError::Rollout(error.to_string()))?;
        self.runtime
            .commit_envelope(envelope)
            .await
            .map_err(|error| StorageError::Recording(error.to_string()))
    }

    async fn stream_next(&mut self) -> Option<Result<Bytes, io::Error>> {
        self.stream.next().await
    }
}

pub(super) async fn forward_journal_stream(
    mut state: JournalStreamState,
    first: Result<Bytes, io::Error>,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
) {
    let mut sender = Some(sender);
    let mut drain_deadline = None;
    let first_failed = first.is_err();
    state.observe_item(&first);

    if !first_failed && state.stream_completed() {
        if let Err(error) = state.finish_success().await {
            send_journal_error(&mut sender, error).await;
            return;
        }
    }
    if let Some(current_sender) = sender.as_ref() {
        if current_sender.send(first).await.is_err() {
            sender = None;
            drain_deadline = Some(tokio::time::Instant::now() + DOWNSTREAM_DRAIN_TIMEOUT);
        }
    }
    if first_failed {
        let (http_status, error_code, error_message) = state.failure.clone().unwrap_or((
            502,
            "stream_error".to_string(),
            "upstream stream failed".to_string(),
        ));
        let _ = state
            .finish_failed(http_status, &error_code, &error_message)
            .await;
        return;
    }

    loop {
        let next = match drain_deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, state.stream_next()).await {
                Ok(item) => item,
                Err(_) => {
                    let _ = state
                        .finish_failed(
                            499,
                            "downstream_disconnect",
                            "downstream client disconnected",
                        )
                        .await;
                    return;
                }
            },
            None => state.stream_next().await,
        };
        let Some(item) = next else {
            if drain_deadline.is_some() {
                let _ = state
                    .finish_failed(
                        499,
                        "downstream_disconnect",
                        "downstream client disconnected",
                    )
                    .await;
            } else if state.finished {
                return;
            } else if state.stream_completed() && !state.failed {
                let _ = state.finish_success().await;
            } else {
                let (http_status, error_code, error_message) = state.failure.clone().unwrap_or((
                    502,
                    "stream_error".to_string(),
                    "upstream stream ended before a completion event".to_string(),
                ));
                let _ = state
                    .finish_failed(http_status, &error_code, &error_message)
                    .await;
            }
            return;
        };
        let item_failed = item.is_err();
        state.observe_item(&item);
        if !item_failed && state.stream_completed() && !state.finished {
            if let Err(error) = state.finish_success().await {
                send_journal_error(&mut sender, error).await;
                return;
            }
        }
        if let Some(current_sender) = sender.as_ref() {
            if current_sender.send(item).await.is_err() {
                sender = None;
                drain_deadline = Some(tokio::time::Instant::now() + DOWNSTREAM_DRAIN_TIMEOUT);
            }
        }
        if item_failed {
            let (http_status, error_code, error_message) = state.failure.clone().unwrap_or((
                502,
                "stream_error".to_string(),
                "upstream stream failed".to_string(),
            ));
            let _ = state
                .finish_failed(http_status, &error_code, &error_message)
                .await;
            return;
        }
    }
}

async fn send_journal_error(
    sender: &mut Option<mpsc::Sender<Result<Bytes, io::Error>>>,
    error: StorageError,
) {
    tracing::error!(
        error = %error,
        "recording commit failed before the stream completion event"
    );
    let Some(current_sender) = sender.as_ref() else {
        return;
    };
    let _ = current_sender
        .send(Err(io::Error::other("recording service unavailable")))
        .await;
}

fn stream_update(
    status: &str,
    http_status: u16,
    error_code: Option<String>,
    error_message: Option<String>,
    snapshot: &StreamStatsSnapshot,
    started_at: Instant,
    upstream_request_id: Option<String>,
) -> StreamActivityUpdate {
    StreamActivityUpdate {
        status: status.to_string(),
        http_status: i64::from(http_status),
        error_code,
        error_message,
        input_tokens: snapshot.input_tokens,
        output_tokens: snapshot.output_tokens,
        reasoning_tokens: None,
        cache_read_tokens: snapshot.cache_read_tokens,
        cache_write_tokens: snapshot.cache_write_tokens,
        latency_ms: started_at.elapsed().as_millis() as i64,
        tool_call_count: Some(snapshot.tool_call_count),
        upstream_request_id,
    }
}

pub(super) fn timed_events(
    events: Vec<ContentDraft>,
    observed_at: &str,
) -> Vec<RecordedContentEvent> {
    events
        .into_iter()
        .map(|event| RecordedContentEvent::new(observed_at, event))
        .collect()
}
