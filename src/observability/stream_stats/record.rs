use std::time::Instant;

use bytes::Bytes;
use futures::Stream;
use uuid::Uuid;

use crate::{bridge::stream::SharedStreamStats, stats::RequestLogInsert, storage::Storage};

use super::state::record_stream_with_log_id;

pub fn record_first_chunk<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: RequestLogInsert,
    started_at: Instant,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_optional_stats(storage, identity_id, stream, log, started_at, None)
}

pub fn record_stream<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: RequestLogInsert,
    started_at: Instant,
    stats: SharedStreamStats,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_optional_stats(storage, identity_id, stream, log, started_at, Some(stats))
}

fn record_stream_with_optional_stats<S>(
    storage: Storage,
    identity_id: String,
    stream: S,
    log: RequestLogInsert,
    started_at: Instant,
    stats: Option<SharedStreamStats>,
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
        stats,
        Uuid::new_v4().to_string(),
    )
}
