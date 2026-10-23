use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::{stream, StreamExt, TryStreamExt};
use serde_json::json;

use super::support::{latest_activity, request_events, test_log, test_storage};
use crate::{
    activity::{RawStreamContentCapture, RawStreamProtocol, SessionMetaEvent},
    bridge::stream::{InternalStreamEvent, StreamStatsSnapshot},
    observability::stream_stats::{
        options::{RecordingMode, StreamRecordOptions},
        record_first_chunk_with_rollout, record_stream_with_rollout,
        state::prepare_stream_with_log_id,
        StreamRecording, StreamRollout,
    },
    storage::StorageError,
};

#[tokio::test]
async fn first_chunk_error_finalizes_a_failed_rollout() {
    let (storage, identity_id, rollout_store) = test_storage().await;
    let mut recorded = record_first_chunk_with_rollout(
        rollout_store.clone(),
        identity_id.clone(),
        stream::iter([Err::<Bytes, _>(std::io::Error::other("upstream failed"))]),
        test_log(),
        std::time::Instant::now(),
        RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
        StreamRollout {
            request_events: request_events("stream input"),
            session_id: None,
        },
    )
    .await
    .expect("prepare failed stream");

    assert!(recorded.next().await.expect("first item").is_err());
    drop(recorded);

    let activity = latest_activity(&storage, &identity_id).await;
    assert_eq!(activity.status, "failed");
    let rollout = storage
        .find_activity_rollout(&activity.id)
        .await
        .expect("load rollout index")
        .expect("rollout index exists");
    assert_eq!(rollout.status, "failed");
    assert!(!rollout_store
        .rollouts()
        .absolute_path(&format!("{}.part", rollout.rollout_path))
        .exists());
    let events = rollout_store
        .rollouts()
        .read_events(&rollout.rollout_path)
        .expect("read failed rollout");
    assert!(events.iter().any(|event| event.event_type == "error"));
}

#[tokio::test]
async fn stream_without_a_completion_event_is_failed() {
    let (storage, identity_id, activity_store) = test_storage().await;
    let mut snapshot = StreamStatsSnapshot::default();
    snapshot.record_event(&InternalStreamEvent::TextDelta(
        "partial output".to_string(),
    ));
    let stats = Arc::new(Mutex::new(snapshot));

    record_stream_with_rollout(
        activity_store.clone(),
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
            b"data: partial\n\n",
        ))]),
        test_log(),
        std::time::Instant::now(),
        StreamRecording {
            stats,
            rollout: StreamRollout {
                request_events: request_events("stream input"),
                session_id: None,
            },
            session: None,
        },
    )
    .await
    .expect("prepare incomplete stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect incomplete stream");

    let activity = latest_activity(&storage, &identity_id).await;
    assert_eq!(activity.status, "failed");
    let rollout = storage
        .find_activity_rollout(&activity.id)
        .await
        .expect("load incomplete rollout")
        .expect("incomplete rollout exists");
    assert_eq!(rollout.status, "failed");
    let events = activity_store
        .rollouts()
        .read_events(&rollout.rollout_path)
        .expect("read incomplete rollout");
    assert!(events.iter().any(|event| event.event_type == "error"));
}

#[tokio::test]
async fn chat_stream_without_done_marker_is_failed() {
    let (storage, identity_id, activity_store) = test_storage().await;
    record_first_chunk_with_rollout(
        activity_store,
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"cut off\"}}]}\n\n",
        ))]),
        test_log(),
        std::time::Instant::now(),
        RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
        StreamRollout {
            request_events: request_events("stream input"),
            session_id: None,
        },
    )
    .await
    .expect("prepare cut-off chat stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect cut-off chat stream");

    let activity = latest_activity(&storage, &identity_id).await;
    assert_eq!(activity.status, "failed");
    assert_eq!(
        activity.error_message.as_deref(),
        Some("upstream stream ended before a completion event")
    );
}

#[tokio::test]
async fn image_stream_without_completion_event_is_failed() {
    let (storage, identity_id, activity_store) = test_storage().await;
    record_first_chunk_with_rollout(
        activity_store,
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
            b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"aGVs\"}\n\n",
        ))]),
        test_log(),
        std::time::Instant::now(),
        RawStreamContentCapture::new(RawStreamProtocol::ImageGeneration),
        StreamRollout {
            request_events: request_events("image input"),
            session_id: None,
        },
    )
    .await
    .expect("prepare cut-off image stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect cut-off image stream");

    let activity = latest_activity(&storage, &identity_id).await;
    assert_eq!(activity.status, "failed");
    assert_eq!(
        activity.error_message.as_deref(),
        Some("upstream stream ended before a completion event")
    );
}

#[tokio::test]
async fn empty_stream_records_a_failed_rollout() {
    let (storage, identity_id, rollout_store) = test_storage().await;
    record_first_chunk_with_rollout(
        rollout_store.clone(),
        identity_id.clone(),
        stream::empty::<Result<Bytes, std::io::Error>>(),
        test_log(),
        std::time::Instant::now(),
        RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
        StreamRollout {
            request_events: request_events("empty input"),
            session_id: None,
        },
    )
    .await
    .expect("prepare empty stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect empty stream");

    let activity = latest_activity(&storage, &identity_id).await;
    assert_eq!(activity.status, "failed");
    let rollout = storage
        .find_activity_rollout(&activity.id)
        .await
        .expect("load rollout index")
        .expect("rollout index exists");
    assert_eq!(rollout.status, "failed");
    let events = rollout_store
        .rollouts()
        .read_events(&rollout.rollout_path)
        .expect("read empty rollout");
    assert!(events.iter().any(|event| {
        event.event_type == "error" && event.payload["code"] == json!("empty_stream")
    }));
}

#[tokio::test]
async fn stream_insert_failure_does_not_update_existing_activity() {
    let (storage, identity_id, rollout_store) = test_storage().await;
    storage
        .insert_activity_with_id(&identity_id, "duplicate-stream-log".to_string(), test_log())
        .await
        .expect("insert existing log");
    let writer = rollout_store
        .rollouts()
        .begin(
            SessionMetaEvent {
                activity_id: "duplicate-stream-log".to_string(),
                session_id: "duplicate-stream-log".to_string(),
                identity_id: identity_id.clone(),
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: Some("chat_completions".to_string()),
                endpoint_name: String::new(),
                model_requested: "coder".to_string(),
                model_upstream: "deepseek-chat".to_string(),
                started_at: "2026-10-22T00:00:00Z".to_string(),
            },
            Vec::new(),
        )
        .expect("begin duplicate rollout");

    let result = prepare_stream_with_log_id(
        rollout_store,
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))]),
        test_log(),
        std::time::Instant::now(),
        StreamRecordOptions {
            stats: None,
            content_capture: None,
            rollout: Some(writer),
            session: None,
            log_id: "duplicate-stream-log".to_string(),
            mode: RecordingMode::Required,
        },
    )
    .await;

    assert!(matches!(result, Err(StorageError::Database(_))));
    let logs = storage
        .list_activities(&identity_id, 10)
        .await
        .expect("load duplicate log");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].input_tokens, None);
}
