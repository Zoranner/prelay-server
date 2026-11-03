use crate::{
    codec::{
        bridge::stream::StreamStatsSnapshot,
        internal::{
            InternalContentPart, InternalOutputItem, InternalResponse, InternalRole, InternalUsage,
        },
    },
    service::activity::ActivityStore,
    service::recording::options::StreamSession,
    store::storage::ResponseStateInsert,
};
use serde_json::Value;

pub(super) async fn save_stream_response_state(
    activity_store: &ActivityStore,
    identity_id: &str,
    session: StreamSession,
    response: InternalResponse,
    snapshot: &StreamStatsSnapshot,
) {
    let request_items = Value::Array(session.request_items.clone());
    let response_items = Value::Array(snapshot.response_output_items.clone());
    let insert = ResponseStateInsert {
        identity_id,
        response_id: &session.response_id,
        previous_response_id: session.previous_response_id.as_deref(),
        conversation_id: &session.conversation_id,
        provider_id: &session.provider_id,
        model: &session.model,
        upstream_response_id: snapshot.upstream_response_id.as_deref(),
        request_items: &request_items,
        response_items: &response_items,
        input_messages: &session.input_messages,
        response: &response,
    };
    if let Err(error) = activity_store.save_response_state(insert).await {
        tracing::error!(
            operation = "save_stream_response_state",
            error_code = error.code().as_str(),
            "streamed response was delivered but its exact state was not persisted"
        );
    }
}

pub(super) async fn save_stream_state_if_ready(
    session_slot: &mut Option<StreamSession>,
    activity_store: &ActivityStore,
    identity_id: &str,
    snapshot: &StreamStatsSnapshot,
) {
    let Some(session) = session_slot.as_ref() else {
        return;
    };
    if !snapshot.completed {
        return;
    }
    if snapshot.response_output_items.is_empty() {
        tracing::error!(
            operation = "save_stream_response_state",
            response_id = %session.response_id,
            "stream completed without exact output items; response state will not be persisted"
        );
        *session_slot = None;
        return;
    }
    let Some(response) = response_from_stream_session(session, snapshot) else {
        return;
    };
    let session = session_slot.take().expect("session checked above");
    save_stream_response_state(activity_store, identity_id, session, response, snapshot).await;
}

pub(super) fn response_from_stream_session(
    session: &StreamSession,
    snapshot: &StreamStatsSnapshot,
) -> Option<InternalResponse> {
    if !snapshot.completed {
        return None;
    }
    if session.response_id.trim().is_empty() {
        return None;
    }

    let mut output = Vec::new();
    if !snapshot.output_text.is_empty() {
        output.push(InternalOutputItem::Message {
            id: "msg_0".to_string(),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text(snapshot.output_text.clone())],
        });
    }
    for (id, name, arguments) in snapshot.completed_tool_calls() {
        if id.is_empty() || name.is_empty() {
            continue;
        }
        output.push(InternalOutputItem::FunctionToolCall {
            id,
            name,
            arguments,
            reasoning_content: None,
        });
    }

    Some(InternalResponse {
        id: session.response_id.clone(),
        model: session.model.clone(),
        output,
        usage: Some(InternalUsage {
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: None,
            cache_read_tokens: snapshot.cache_read_tokens,
            cache_write_tokens: snapshot.cache_write_tokens,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::response_from_stream_session;
    use crate::{
        codec::{
            bridge::stream::{
                InternalFinishReason, InternalStreamEvent, StreamStatsSnapshot, StreamUsage,
            },
            internal::{InternalContentPart, InternalOutputItem, InternalRole},
        },
        service::recording::options::StreamSession,
    };

    fn completed_snapshot() -> StreamStatsSnapshot {
        let mut snapshot = StreamStatsSnapshot {
            upstream_response_id: Some("resp_upstream".to_string()),
            response_output_items: vec![serde_json::json!({
                "type": "message",
                "id": "msg_0",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "hello"}]
            })],
            ..Default::default()
        };
        snapshot.record_event(&InternalStreamEvent::TextDelta("hello".to_string()));
        snapshot.record_event(&InternalStreamEvent::ToolCallDone {
            index: 0,
            id: "call_1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"a.txt"}"#.to_string(),
        });
        snapshot.record_event(&InternalStreamEvent::Usage(StreamUsage {
            input_tokens: Some(10),
            output_tokens: Some(4),
            total_tokens: Some(14),
            cache_read_tokens: Some(2),
            cache_write_tokens: Some(1),
        }));
        snapshot.record_event(&InternalStreamEvent::Finished(InternalFinishReason::Stop));
        snapshot
    }

    fn session() -> StreamSession {
        StreamSession {
            previous_response_id: Some("resp_previous".to_string()),
            conversation_id: "conv_test".to_string(),
            provider_id: "provider-a".to_string(),
            model: "deepseek-flash".to_string(),
            input_messages: Vec::new(),
            request_items: Vec::new(),
            response_id: "resp_local".to_string(),
        }
    }

    #[test]
    fn builds_a_complete_response_from_stream_stats() {
        let response =
            response_from_stream_session(&session(), &completed_snapshot()).expect("response");

        assert_eq!(response.id, "resp_local");
        assert_eq!(response.model, "deepseek-flash");
        assert_eq!(
            response.output,
            vec![
                InternalOutputItem::Message {
                    id: "msg_0".to_string(),
                    role: InternalRole::Assistant,
                    content: vec![InternalContentPart::Text("hello".to_string())],
                },
                InternalOutputItem::FunctionToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"a.txt"}"#.to_string(),
                    reasoning_content: None,
                },
            ]
        );
        assert_eq!(
            response.usage.and_then(|usage| usage.input_tokens),
            Some(10)
        );
    }

    #[test]
    fn never_uses_the_upstream_response_id_as_the_local_id() {
        let response =
            response_from_stream_session(&session(), &completed_snapshot()).expect("response");

        assert_ne!(response.id, "resp_upstream");
    }

    #[test]
    fn does_not_build_a_session_for_an_incomplete_stream() {
        let mut snapshot = completed_snapshot();
        snapshot.completed = false;

        assert!(response_from_stream_session(&session(), &snapshot).is_none());
    }
}
