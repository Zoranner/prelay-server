use crate::{routes::v1::auth::CurrentProtocolAccess, stats::ActivityInsert, AppState};

use super::IMAGE_GENERATIONS_PROTOCOL;

pub(super) struct ImageActivityParams<'a> {
    pub(super) access: &'a CurrentProtocolAccess,
    pub(super) provider: &'a crate::models::ProviderConfig,
    pub(super) model_requested: String,
    pub(super) model_upstream: String,
    pub(super) status: &'a str,
    pub(super) http_status: i64,
    pub(super) error_code: Option<&'a str>,
    pub(super) is_streaming: bool,
    pub(super) latency_ms: i64,
    pub(super) upstream_latency_ms: Option<i64>,
    pub(super) upstream_request_id: Option<String>,
    pub(super) error_message: Option<String>,
}

pub(super) fn image_activity(params: ImageActivityParams<'_>) -> ActivityInsert {
    ActivityInsert {
        protocol_in: IMAGE_GENERATIONS_PROTOCOL.to_string(),
        protocol_out: IMAGE_GENERATIONS_PROTOCOL.to_string(),
        protocol_upstream: IMAGE_GENERATIONS_PROTOCOL.to_string(),
        provider_id: params.provider.id.clone(),
        provider_name: params.provider.name.clone(),
        endpoint_name: params.access.endpoint_name.clone(),
        model_requested: params.model_requested,
        model_upstream: params.model_upstream,
        status: params.status.to_string(),
        http_status: params.http_status,
        error_code: params.error_code.map(str::to_string),
        error_message: params.error_message,
        is_streaming: params.is_streaming,
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms: params.latency_ms,
        upstream_latency_ms: params.upstream_latency_ms,
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: params.upstream_request_id,
        metadata_json: None,
    }
}

pub(super) async fn insert_image_activity_best_effort(
    state: &AppState,
    access: &CurrentProtocolAccess,
    log: ActivityInsert,
) {
    let _ = insert_image_activity_with_id_best_effort(state, access, log).await;
}

pub(super) async fn insert_image_activity_with_id_best_effort(
    state: &AppState,
    access: &CurrentProtocolAccess,
    log: ActivityInsert,
) -> Option<String> {
    match state
        .storage
        .insert_activity(&access.identity_id, log)
        .await
    {
        Ok(activity_id) => Some(activity_id),
        Err(_) => {
            tracing::warn!(
                failure_kind = "image_activity_storage",
                "failed to persist image activity"
            );
            None
        }
    }
}
