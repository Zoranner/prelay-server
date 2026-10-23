use std::{
    pin::Pin,
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use tokio::sync::mpsc;

use crate::{activity::ActivityStore, stats::ActivityInsert, storage::StorageError};

use super::StreamRecordState;
use crate::observability::stream_stats::options::StreamRecordOptions;

pub(crate) type RecordedStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;
const DOWNSTREAM_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const RECORDED_STREAM_CHANNEL_CAPACITY: usize = 16;

pub(crate) async fn prepare_stream_with_log_id<S>(
    activity_store: ActivityStore,
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
        activity_store,
        identity_id,
        stream: upstream,
        log: Some(log),
        log_id: options.log_id,
        started_at,
        stats: options.stats,
        content_capture: options.content_capture,
        rollout: options.rollout,
        session: options.session,
        mode: options.mode,
        upstream_request_id: None,
        inserted: false,
        failed: false,
        usage_recorded: false,
        rollout_finalized: false,
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
