use crate::{
    activity::ActivityStore,
    bridge::{
        internal::{
            InternalContentPart, InternalOutputItem, InternalResponse, InternalRole, InternalUsage,
        },
        stream::StreamStatsSnapshot,
    },
    observability::stream_stats::options::StreamSession,
    storage::ResponseSessionInsert,
};

pub(super) async fn save_response_session(
    activity_store: &ActivityStore,
    identity_id: &str,
    session: StreamSession,
    response: InternalResponse,
) {
    let insert = ResponseSessionInsert {
        identity_id,
        response_id: &response.id,
        previous_response_id: session.previous_response_id.as_deref(),
        conversation_id: &session.conversation_id,
        provider_id: &session.provider_id,
        model: &session.model,
        input_messages: &session.input_messages,
        response: &response,
    };
    if let Err(error) = activity_store.save_response_session(insert).await {
        tracing::error!(
            operation = "save_stream_response_session",
            error_code = error.code().as_str(),
            "failed to persist streamed response session"
        );
    }
}

pub(super) fn response_from_stream_session(
    session: &StreamSession,
    snapshot: &StreamStatsSnapshot,
) -> Option<InternalResponse> {
    if !snapshot.completed {
        return None;
    }
    let response_id = snapshot
        .response_id
        .as_deref()
        .or(session.response_id.as_deref())
        .filter(|id| !id.trim().is_empty())?
        .to_string();

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
        id: response_id,
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
        bridge::{
            internal::{InternalContentPart, InternalOutputItem, InternalRole},
            stream::{InternalFinishReason, InternalStreamEvent, StreamStatsSnapshot, StreamUsage},
        },
        observability::stream_stats::options::StreamSession,
    };

    fn completed_snapshot() -> StreamStatsSnapshot {
        let mut snapshot = StreamStatsSnapshot {
            response_id: Some("resp_upstream".to_string()),
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
            store: true,
            response_id: Some("resp_local".to_string()),
        }
    }

    #[test]
    fn builds_a_complete_response_from_stream_stats() {
        let response =
            response_from_stream_session(&session(), &completed_snapshot()).expect("response");

        assert_eq!(response.id, "resp_upstream");
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
    fn uses_the_bridge_response_id_when_the_stream_has_none() {
        let mut snapshot = completed_snapshot();
        snapshot.response_id = None;

        let response = response_from_stream_session(&session(), &snapshot).expect("response");

        assert_eq!(response.id, "resp_local");
    }

    #[test]
    fn does_not_build_a_session_for_an_incomplete_stream() {
        let mut snapshot = completed_snapshot();
        snapshot.completed = false;

        assert!(response_from_stream_session(&session(), &snapshot).is_none());
    }
}
