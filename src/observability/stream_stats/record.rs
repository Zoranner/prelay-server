use std::time::Instant;

use bytes::Bytes;
use futures::Stream;
use uuid::Uuid;

use crate::{
    activity::RawStreamContentCapture,
    bridge::stream::SharedStreamStats,
    stats::ActivityInsert,
    storage::{Storage, StorageError},
};

use super::state::{prepare_stream_with_log_id, RecordedStream, StreamRecordOptions};

pub async fn record_first_chunk<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    prepare_stream(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions::required(None, String::new(), None),
    )
    .await
}

pub async fn record_first_chunk_with_activity_content<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    input_text: String,
    content_capture: RawStreamContentCapture,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    prepare_stream(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions::required(None, input_text, Some(content_capture)),
    )
    .await
}

pub async fn record_first_chunk_with_activity_content_best_effort<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    input_text: String,
    content_capture: RawStreamContentCapture,
) -> RecordedStream
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    prepare_stream(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions::best_effort(None, input_text, Some(content_capture)),
    )
    .await
    .expect("best-effort recording never returns a storage error")
}

pub async fn record_stream<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    stats: SharedStreamStats,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_activity_content(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        stats,
        String::new(),
    )
    .await
}

pub async fn record_stream_with_activity_content<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    stats: SharedStreamStats,
    input_text: String,
) -> Result<RecordedStream, StorageError>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    prepare_stream(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions::required(Some(stats), input_text, None),
    )
    .await
}

async fn prepare_stream<S>(
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
    prepare_stream_with_log_id(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions {
            log_id: Uuid::new_v4().to_string(),
            ..options
        },
    )
    .await
}
