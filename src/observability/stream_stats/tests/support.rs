use serde_json::json;

use crate::{
    activity::{EventPhase, MessageEvent, RolloutEventBody},
    stats::ActivityInsert,
    storage::Storage,
};

pub(super) async fn test_storage() -> (Storage, String, crate::activity::ActivityStore) {
    let state = crate::test_support::test_state().await;
    let storage = state.storage;
    let identity = storage
        .register_identity(
            "machine-1",
            "S-1-5-21-1",
            &crate::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    (storage, identity.identity_id, state.activity_store)
}

pub(super) async fn latest_activity(
    storage: &Storage,
    identity_id: &str,
) -> crate::stats::ActivitySummary {
    storage
        .list_activities(identity_id, 10)
        .await
        .expect("load stream activity")
        .pop()
        .expect("stored activity")
}

pub(super) fn request_events(content: &str) -> Vec<RolloutEventBody> {
    vec![RolloutEventBody::Message(MessageEvent {
        phase: EventPhase::Request,
        role: "user".to_string(),
        content: json!(content),
    })]
}

pub(super) fn test_log() -> ActivityInsert {
    ActivityInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: "chat_completions".to_string(),
        endpoint_name: String::new(),
        provider_id: "provider-1".to_string(),
        provider_name: "DeepSeek".to_string(),
        provider_type: "deepseek".to_string(),
        model_requested: "coder".to_string(),
        model_upstream: "deepseek-chat".to_string(),
        status: "success".to_string(),
        http_status: 200,
        error_code: None,
        error_message: None,
        is_streaming: true,
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms: 0,
        upstream_latency_ms: Some(5),
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: Some("req_123".to_string()),
    }
}
