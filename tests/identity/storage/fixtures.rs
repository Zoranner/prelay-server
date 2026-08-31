use prelay_protocol::CreateProviderRequest;
use prelay_server::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalResponse, InternalRole,
    },
    identity::credential::generate_credential,
    stats::ActivityInsert,
    storage::Storage,
};

pub(super) fn provider_input(name: &str, api_key: &str) -> CreateProviderRequest {
    CreateProviderRequest {
        name: name.to_string(),
        provider_type: "openai_compatible".to_string(),
        base_url: "https://provider.example".to_string(),
        api_key: api_key.to_string(),
        capabilities: None,
        models: vec!["test-model".to_string()],
    }
}

pub(super) fn activity(provider_id: &str, upstream_latency_ms: i64) -> ActivityInsert {
    ActivityInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: "chat_completions".to_string(),
        endpoint_name: "Test endpoint".to_string(),
        provider_id: provider_id.to_string(),
        provider_name: "Test provider".to_string(),
        model_requested: "shared-model".to_string(),
        model_upstream: "test-model".to_string(),
        status: "success".to_string(),
        http_status: 200,
        latency_ms: upstream_latency_ms,
        upstream_latency_ms: Some(upstream_latency_ms),
        ..Default::default()
    }
}

pub(super) async fn seed_identity_and_provider(
    storage: &Storage,
    suffix: &str,
) -> (String, String) {
    let identity = storage
        .register_identity(
            &format!("machine-{suffix}"),
            &format!("sid-{suffix}"),
            &generate_credential(),
        )
        .await
        .expect("register identity");
    let provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_input(&format!("Provider {suffix}"), &format!("test-{suffix}-key")),
        )
        .await
        .expect("create provider");
    (identity.identity_id, provider_id)
}

pub(super) fn message(role: InternalRole, text: &str) -> InternalMessage {
    InternalMessage {
        role,
        content: vec![InternalContentPart::Text(text.to_string())],
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    }
}

pub(super) fn response(id: &str, text: &str) -> InternalResponse {
    InternalResponse {
        id: id.to_string(),
        model: "test-model".to_string(),
        output: vec![InternalOutputItem::Message {
            id: format!("{id}-message"),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text(text.to_string())],
        }],
        usage: None,
    }
}
