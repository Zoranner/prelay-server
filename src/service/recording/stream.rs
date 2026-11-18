use std::{io, time::Instant};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    codec::bridge::stream::SharedStreamStats,
    domain::{
        activity::rollout::SessionContext,
        activity::{ContentDraft, RawStreamContentCapture},
        stats::ActivityInsert,
    },
    service::recording::{
        options::StreamSession,
        record::{
            record_first_chunk_with_rollout_best_effort, record_stream_with_rollout,
            StreamRecording, StreamRollout,
        },
        state::RecordedStream,
        write::{record_completed_turn, CompletedTurnRecord},
    },
    store::{
        recording_journal::{RecordEnvelope, RecordKind, TurnStartedRecord, TurnTiming},
        storage::StorageError,
    },
    AppState,
};

mod forward;

use self::forward::{forward_journal_stream, timed_events, JournalStreamState};

pub(crate) async fn record_stream_with_journal<S>(
    state: &AppState,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    started_at_wall: DateTime<Utc>,
    recording: StreamRecording,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let StreamRecording {
        stats,
        rollout,
        session,
    } = recording;
    let StreamRollout {
        request_events,
        session: session_context,
    } = rollout;

    if state.recording.is_none() {
        return record_stream_with_rollout(
            state.activity_store.clone(),
            identity_id,
            stream,
            log,
            started_at,
            StreamRecording {
                stats,
                rollout: StreamRollout {
                    request_events,
                    session: session_context,
                },
                session,
            },
        )
        .await;
    }

    prepare_journal_stream(
        state,
        stream,
        JournalStreamInput {
            identity_id,
            log,
            started_at,
            started_at_wall,
            stats: Some(stats),
            content_capture: None,
            response_session: session,
            session_context,
            request_events,
        },
    )
    .await
}

pub(crate) async fn record_image_stream_with_journal<S>(
    state: &AppState,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    started_at_wall: DateTime<Utc>,
    recording: ImageStreamRecording,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let ImageStreamRecording {
        content_capture,
        rollout,
    } = recording;
    if state.recording.is_none() {
        return Ok(record_first_chunk_with_rollout_best_effort(
            state.activity_store.clone(),
            identity_id,
            stream,
            log,
            started_at,
            content_capture,
            rollout,
        )
        .await);
    }

    let StreamRollout {
        request_events,
        session: session_context,
    } = rollout;
    prepare_journal_stream(
        state,
        stream,
        JournalStreamInput {
            identity_id,
            log,
            started_at,
            started_at_wall,
            stats: None,
            content_capture: Some(content_capture),
            response_session: None,
            session_context,
            request_events,
        },
    )
    .await
}

pub(crate) struct ImageStreamRecording {
    pub(crate) content_capture: RawStreamContentCapture,
    pub(crate) rollout: StreamRollout,
}

async fn prepare_journal_stream<S>(
    state: &AppState,
    stream: S,
    input: JournalStreamInput,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let JournalStreamInput {
        identity_id,
        mut log,
        started_at,
        started_at_wall,
        stats,
        content_capture,
        response_session,
        session_context,
        request_events,
    } = input;
    let runtime = state
        .recording
        .as_ref()
        .ok_or_else(|| StorageError::Rollout("recording runtime is not available".to_string()))?
        .clone();
    let mut upstream = Box::pin(stream);
    let first = upstream.next().await;
    let Some(first) = first else {
        log.status = "failed".to_string();
        log.http_status = 502;
        log.latency_ms = started_at.elapsed().as_millis() as i64;
        log.error_code = Some("empty_stream".to_string());
        log.error_message = Some("upstream stream finished without chunks".to_string());
        record_completed_turn(
            state,
            CompletedTurnRecord {
                identity_id: identity_id.clone(),
                activity: log,
                session: session_context.clone(),
                request_events,
                response_events: Vec::new(),
                response_state: None,
                started_at: started_at_wall,
                finished_at: Utc::now(),
            },
        )
        .await?;
        return Ok(Box::pin(futures::stream::empty()));
    };

    let first_is_ok = first.is_ok();
    let first_token_at = first_is_ok.then(|| Utc::now().to_rfc3339());
    let first_token_ms = started_at.elapsed().as_millis() as i64;
    if first_is_ok {
        log.status = "streaming".to_string();
        log.first_token_ms = Some(first_token_ms);
        log.latency_ms = first_token_ms;
    } else {
        log.status = "failed".to_string();
        log.http_status = 502;
        log.first_token_ms = None;
        log.latency_ms = first_token_ms;
        log.error_code = Some("stream_error".to_string());
        log.error_message = first.as_ref().err().map(ToString::to_string);
    }

    let activity_id = Uuid::new_v4().to_string();
    let started_at_string = started_at_wall.to_rfc3339();
    let payload = TurnStartedRecord {
        activity: log.clone(),
        session: session_context.clone(),
        request_events: timed_events(request_events, &started_at_string),
        timing: TurnTiming {
            started_at: started_at_string.clone(),
            first_token_at: first_token_at.clone(),
            finished_at: None,
        },
    };
    let envelope = RecordEnvelope::from_payload(
        activity_id.clone(),
        identity_id.clone(),
        RecordKind::TurnStarted,
        started_at_string,
        &payload,
    )
    .map_err(|error| StorageError::Rollout(error.to_string()))?;
    runtime
        .commit_envelope(envelope)
        .await
        .map_err(|error| StorageError::Recording(error.to_string()))?;

    let stream_state = JournalStreamState {
        runtime,
        identity_id,
        activity_id,
        stream: upstream,
        started_at,
        started_at_wall,
        first_token_at,
        upstream_request_id: log.upstream_request_id.clone(),
        stats,
        content_capture,
        response_session,
        session_context,
        failed: !first_is_ok,
        failure: first
            .as_ref()
            .err()
            .map(|error| (502, "stream_error".to_string(), error.to_string())),
        finished: false,
    };
    let (sender, receiver) = mpsc::channel(forward::CHANNEL_CAPACITY);
    tokio::spawn(forward_journal_stream(stream_state, first, sender));
    Ok(Box::pin(futures::stream::unfold(
        receiver,
        |mut receiver| async move { receiver.recv().await.map(|item| (item, receiver)) },
    )))
}

struct JournalStreamInput {
    identity_id: String,
    log: ActivityInsert,
    started_at: Instant,
    started_at_wall: DateTime<Utc>,
    stats: Option<SharedStreamStats>,
    content_capture: Option<RawStreamContentCapture>,
    response_session: Option<StreamSession>,
    session_context: SessionContext,
    request_events: Vec<ContentDraft>,
}
