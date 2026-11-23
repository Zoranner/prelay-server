use crate::{
    codec::bridge::stream::StreamStatsSnapshot, service::activity::ActivityStore,
    service::recording::options::StreamSession, store::storage::ResponseStateInsert,
};
use serde_json::Value;

pub(super) async fn save_stream_response_state(
    activity_store: &ActivityStore,
    identity_id: &str,
    session: StreamSession,
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
    let session = session_slot.take().expect("session checked above");
    save_stream_response_state(activity_store, identity_id, session, snapshot).await;
}
