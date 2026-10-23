use std::time::Instant;

use bytes::Bytes;
use futures::Stream;

use crate::{
    activity::{ActivityStore, RawStreamContentCapture, RolloutEventBody},
    bridge::stream::SharedStreamStats,
    stats::ActivityInsert,
    storage::StorageError,
};

use super::{
    options::{RecordingMode, StreamRecordOptions, StreamSession},
    state::{prepare_stream_with_log_id, RecordedStream},
};

pub struct StreamRollout<'a> {
    pub request_events: Vec<RolloutEventBody>,
    pub session_id: Option<&'a str>,
}

pub(crate) struct StreamRecording<'a> {
    pub(crate) stats: SharedStreamStats,
    pub(crate) rollout: StreamRollout<'a>,
    pub(crate) session: Option<StreamSession>,
}

pub async fn record_first_chunk_with_rollout<S>(
    activity_store: ActivityStore,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    content_capture: RawStreamContentCapture,
    rollout: StreamRollout<'_>,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let StreamRollout {
        request_events,
        session_id,
    } = rollout;
    let (activity_id, writer) =
        activity_store.begin_stream(&identity_id, &log, session_id, request_events)?;
    prepare_stream_with_log_id(
        activity_store,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions {
            stats: None,
            content_capture: Some(content_capture),
            rollout: Some(writer),
            session: None,
            log_id: activity_id,
            mode: RecordingMode::Required,
        },
    )
    .await
}

pub async fn record_first_chunk_with_rollout_best_effort<S>(
    activity_store: ActivityStore,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    content_capture: RawStreamContentCapture,
    rollout: StreamRollout<'_>,
) -> RecordedStream
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let StreamRollout {
        request_events,
        session_id,
    } = rollout;
    let (activity_id, writer) =
        match activity_store.begin_stream(&identity_id, &log, session_id, request_events) {
            Ok(started) => started,
            Err(error) => {
                tracing::warn!(
                    failure_kind = "rollout_begin",
                    error_code = error.code().as_str(),
                    "failed to start best-effort rollout recording"
                );
                return Box::pin(stream);
            }
        };
    match prepare_stream_with_log_id(
        activity_store,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions {
            stats: None,
            content_capture: Some(content_capture),
            rollout: Some(writer),
            session: None,
            log_id: activity_id,
            mode: RecordingMode::BestEffort,
        },
    )
    .await
    {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(
                failure_kind = "rollout_stream",
                error_code = error.code().as_str(),
                "failed to start best-effort stream recording"
            );
            Box::pin(futures::stream::empty())
        }
    }
}

pub(crate) async fn record_stream_with_rollout<S>(
    activity_store: ActivityStore,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    recording: StreamRecording<'_>,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let StreamRecording {
        stats,
        rollout,
        session,
    } = recording;
    let StreamRollout {
        request_events,
        session_id,
    } = rollout;
    let (activity_id, writer) =
        activity_store.begin_stream(&identity_id, &log, session_id, request_events)?;
    prepare_stream_with_log_id(
        activity_store,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions {
            stats: Some(stats),
            content_capture: None,
            rollout: Some(writer),
            session,
            log_id: activity_id,
            mode: RecordingMode::Required,
        },
    )
    .await
}
