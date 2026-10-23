use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::{stream, TryStreamExt};
use prelay_protocol::CreateProviderRequest;

use super::support::{test_log, test_storage};
use crate::{
    bridge::{
        internal::{InternalContentPart, InternalMessage, InternalRole},
        stream::{InternalFinishReason, InternalStreamEvent, StreamStatsSnapshot},
    },
    observability::stream_stats::{
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

    let mut snapshot = StreamStatsSnapshot {
        response_id: Some("resp_stream".to_string()),
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
                session_id: None,
            },
            session: Some(StreamSession {
                previous_response_id: None,
                conversation_id: "conversation-stream".to_string(),
                provider_id,
                model: "deepseek-chat".to_string(),
                input_messages,
                store: true,
                response_id: None,
            }),
        },
    )
    .await
    .expect("prepare stream")
    .try_collect::<Vec<_>>()
    .await
    .expect("collect stream");

    let messages = storage
        .load_response_session_messages(&identity_id, "resp_stream")
        .await
        .expect("load response session")
        .expect("response session exists");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].role, InternalRole::Assistant);
    assert_eq!(
        messages[1].content,
        vec![InternalContentPart::Text("streamed answer".to_string())]
    );
}
