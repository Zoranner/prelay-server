use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::{stream, TryStreamExt};
use serde_json::json;

use super::support::{latest_activity, request_events, test_log, test_storage};
use crate::{
    activity::{RawStreamContentCapture, RawStreamProtocol},
    bridge::stream::{InternalFinishReason, InternalStreamEvent, StreamStatsSnapshot, StreamUsage},
    observability::stream_stats::{
        record_first_chunk_with_rollout, record_stream_with_rollout, StreamRecording, StreamRollout,
    },
};

#[tokio::test]
async fn first_chunk_recording_finalizes_a_rollout_on_eof() {
    let (storage, identity_id, rollout_store) = test_storage().await;
    let chunk = Bytes::from_static(
        b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\ndata: [DONE]\n\n",
    );
    let recorded = record_first_chunk_with_rollout(
        rollout_store.clone(),
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(chunk.clone())]),
        test_log(),
        std::time::Instant::now(),
        RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
        StreamRollout {
            request_events: request_events("stream input"),
            session_id: None,
        },
    )
    .await
    .expect("prepare stream");

    let chunks = recorded
        .try_collect::<Vec<_>>()
        .await
        .expect("collect stream");
    assert_eq!(chunks, vec![chunk]);

    let activity = latest_activity(&storage, &identity_id).await;
    let rollout = storage
        .find_activity_rollout(&activity.id)
        .await
        .expect("load rollout index")
        .expect("rollout index exists");
    assert_eq!(rollout.status, "completed");
    let events = rollout_store
        .rollouts()
        .read_events(&rollout.rollout_path)
        .expect("read rollout events");
    assert_eq!(events[0].event_type, "session_meta");
    assert_eq!(events[1].payload["content"], json!("stream input"));
    assert!(events
        .iter()
        .any(|event| event.event_type == "message" && event.payload["content"] == json!("hello")));
    assert_eq!(
        events.last().map(|event| event.event_type.as_str()),
        Some("session_end")
    );
}

#[tokio::test]
async fn first_chunk_recording_keeps_content_larger_than_the_old_capture_limit() {
    let (storage, identity_id, rollout_store) = test_storage().await;
    let large = "x".repeat(1024 * 1024);
    let data = json!({"choices": [{"delta": {"content": large.clone()}}]}).to_string();
    let chunk = Bytes::from(format!("data: {data}\n\ndata: [DONE]\n\n"));
    record_first_chunk_with_rollout(
        rollout_store.clone(),
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(chunk)]),
        test_log(),
        std::time::Instant::now(),
        RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
        StreamRollout {
            request_events: request_events("large input"),
            session_id: None,
        },
    )
    .await
    .expect("prepare large stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect large stream");

    let activity = latest_activity(&storage, &identity_id).await;
    let rollout = storage
        .find_activity_rollout(&activity.id)
        .await
        .expect("load rollout index")
        .expect("rollout index exists");
    let events = rollout_store
        .rollouts()
        .read_events(&rollout.rollout_path)
        .expect("read large rollout");
    let response = events
        .iter()
        .find(|event| event.event_type == "message" && event.payload["content"] == json!(large))
        .expect("full large response message");
    assert!(response.payload["content"].as_str().unwrap().len() >= 1024 * 1024);
}

#[tokio::test]
async fn stream_recording_updates_usage_and_preserves_tool_calls() {
    let (storage, identity_id, rollout_store) = test_storage().await;
    let mut snapshot = StreamStatsSnapshot::default();
    snapshot.record_event(&InternalStreamEvent::ReasoningDelta(
        "inspect first".to_string(),
    ));
    snapshot.record_event(&InternalStreamEvent::TextDelta("stream output".to_string()));
    snapshot.record_event(&InternalStreamEvent::ToolCallDone {
        index: 0,
        id: "call_1".to_string(),
        name: "read_file".to_string(),
        arguments: "{\"path\":\"src/main.rs\"}".to_string(),
    });
    snapshot.record_event(&InternalStreamEvent::Usage(StreamUsage {
        input_tokens: Some(11),
        output_tokens: Some(7),
        total_tokens: Some(18),
        cache_read_tokens: Some(3),
        cache_write_tokens: Some(2),
    }));
    snapshot.record_event(&InternalStreamEvent::Finished(InternalFinishReason::Stop));
    let stats = Arc::new(Mutex::new(snapshot));

    record_stream_with_rollout(
        rollout_store.clone(),
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))]),
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
    .expect("prepare stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect stream");

    let activity = latest_activity(&storage, &identity_id).await;
    assert_eq!(activity.input_tokens, Some(11));
    assert_eq!(activity.output_tokens, Some(7));
    assert_eq!(activity.cache_read_tokens, Some(3));
    assert_eq!(activity.cache_write_tokens, Some(2));
    let rollout = storage
        .find_activity_rollout(&activity.id)
        .await
        .expect("load rollout index")
        .expect("rollout index exists");
    let events = rollout_store
        .rollouts()
        .read_events(&rollout.rollout_path)
        .expect("read rollout events");
    assert!(events.iter().any(|event| {
        event.event_type == "tool_call"
            && event.payload["tool_call_id"] == json!("call_1")
            && event.payload["arguments"] == json!("{\"path\":\"src/main.rs\"}")
    }));
    assert!(events.iter().any(|event| event.event_type == "usage"));
    assert!(events.iter().any(|event| {
        event.event_type == "reasoning" && event.payload["content"] == json!("inspect first")
    }));
}
