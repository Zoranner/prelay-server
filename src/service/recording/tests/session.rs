use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::{stream, TryStreamExt};
use prelay_protocol::CreateProviderRequest;
use serde_json::json;

use super::support::{test_log, test_storage};
use crate::{
    codec::{
        bridge::stream::{InternalFinishReason, InternalStreamEvent, StreamStatsSnapshot},
        internal::{InternalContentPart, InternalMessage, InternalRole},
    },
    domain::activity::SessionContext,
    service::recording::{
        record_stream_with_rollout, StreamRecording, StreamRollout, StreamSession,
    },
};

#[tokio::test]
async fn stream_recording_persists_a_completed_response_session() {
    let (storage, identity_id, rollout_store) = test_storage().await;
    let provider_id = storage
        .create_provider(
            &identity_id,
            CreateProviderRequest {
                name: "Stream Provider".to_string(),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider.example".to_string(),
                api_key: "test-key".to_string(),
                capabilities: None,
                models: None,
            },
        )
        .await
        .expect("create provider");

    let response_items = vec![json!({
        "type": "message",
        "id": "msg_0",
        "status": "completed",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "streamed answer", "annotations": []}]
    })];
    let mut snapshot = StreamStatsSnapshot {
        upstream_response_id: Some("resp_upstream".to_string()),
        response_output_items: response_items.clone(),
        ..Default::default()
    };
    snapshot.record_event(&InternalStreamEvent::TextDelta(
        "streamed answer".to_string(),
    ));
    snapshot.record_event(&InternalStreamEvent::Finished(InternalFinishReason::Stop));
    let stats = Arc::new(Mutex::new(snapshot));

    let input_messages = vec![InternalMessage {
        role: InternalRole::User,
        content: vec![InternalContentPart::Text("question".to_string())],
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    }];
    record_stream_with_rollout(
        rollout_store,
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"data: {}\n\n"))]),
        test_log(),
        std::time::Instant::now(),
        StreamRecording {
            stats,
            rollout: StreamRollout {
                request_events: Vec::new(),
                session: SessionContext::new("stream-session"),
            },
            session: Some(StreamSession {
                previous_response_id: None,
                conversation_id: "conversation-stream".to_string(),
                provider_id,
                model: "deepseek-chat".to_string(),
                input_messages,
                request_items: vec![json!({"role": "user", "content": "question"})],
                response_id: "resp_stream".to_string(),
            }),
        },
    )
    .await
    .expect("prepare stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect stream");

    let state = storage
        .load_response_state(&identity_id, "resp_stream")
        .await
        .expect("load response state")
        .expect("response state exists");
    assert_eq!(state.upstream_response_id.as_deref(), Some("resp_upstream"));
    assert_eq!(
        state.request_items,
        json!([{"role": "user", "content": "question"}])
    );
    assert_eq!(state.response_items, json!(response_items));
    assert_eq!(
        state.response_items[0]["content"][0]["text"],
        "streamed answer"
    );
}

#[tokio::test]
async fn stream_recording_skips_state_without_exact_output_items() {
    let (storage, identity_id, rollout_store) = test_storage().await;
    let provider_id = storage
        .create_provider(
            &identity_id,
            CreateProviderRequest {
                name: "Incomplete Stream Provider".to_string(),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider.example".to_string(),
                api_key: "test-key".to_string(),
                capabilities: None,
                models: None,
            },
        )
        .await
        .expect("create provider");

    let mut snapshot = StreamStatsSnapshot {
        upstream_response_id: Some("resp_upstream".to_string()),
        ..Default::default()
    };
    snapshot.record_event(&InternalStreamEvent::TextDelta(
        "streamed answer".to_string(),
    ));
    snapshot.record_event(&InternalStreamEvent::Finished(InternalFinishReason::Stop));
    let stats = Arc::new(Mutex::new(snapshot));

    record_stream_with_rollout(
        rollout_store,
        identity_id.clone(),
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"data: {}\n\n"))]),
        test_log(),
        std::time::Instant::now(),
        StreamRecording {
            stats,
            rollout: StreamRollout {
                request_events: Vec::new(),
                session: SessionContext::new("stream-session-incomplete"),
            },
            session: Some(StreamSession {
                previous_response_id: None,
                conversation_id: "conversation-stream".to_string(),
                provider_id,
                model: "deepseek-chat".to_string(),
                input_messages: Vec::new(),
                request_items: vec![json!({"role": "user", "content": "question"})],
                response_id: "resp_incomplete".to_string(),
            }),
        },
    )
    .await
    .expect("prepare stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect stream");

    assert!(storage
        .load_response_state(&identity_id, "resp_incomplete")
        .await
        .expect("load response state")
        .is_none());
}
