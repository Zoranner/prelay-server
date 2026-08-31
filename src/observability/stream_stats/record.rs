use std::time::Instant;

use bytes::Bytes;
use futures::Stream;
use uuid::Uuid;

use crate::{
    activity::RawStreamContentCapture, bridge::stream::SharedStreamStats, stats::ActivityInsert,
    storage::Storage,
};

use super::state::{record_stream_with_log_id, StreamRecordOptions};

pub fn record_first_chunk<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_optional_stats(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions {
            stats: None,
            input_text: String::new(),
            content_capture: None,
            log_id: String::new(),
        },
    )
}

pub fn record_first_chunk_with_activity_content<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    input_text: String,
    content_capture: RawStreamContentCapture,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_optional_stats(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions {
            stats: None,
            input_text,
            content_capture: Some(content_capture),
            log_id: String::new(),
        },
    )
}

pub fn record_stream<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    stats: SharedStreamStats,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
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
}

pub fn record_stream_with_activity_content<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: ActivityInsert,
    started_at: Instant,
    stats: SharedStreamStats,
    input_text: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_optional_stats(
        storage,
        identity_id,
        stream,
        log,
        started_at,
        StreamRecordOptions {
            stats: Some(stats),
            input_text,
            content_capture: None,
            log_id: String::new(),
        },
    )
}

fn record_stream_with_optional_stats<S>(
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
    record_stream_with_log_id(
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
}
