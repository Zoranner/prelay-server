use axum::response::{IntoResponse, Response};
use serde_json::Value;

use crate::{
    error::AppError,
    providers::spec::UpstreamProtocol,
    routes::v1::{auth::CurrentProtocolAccess, endpoint_resolver::ResolvedEndpointProvider},
    AppState,
};

use super::{
    chat::create_chat_anthropic_message, handler::AnthropicMessageCandidateRequest,
    native::create_native_anthropic_message, responses::create_responses_anthropic_message,
};

pub(super) struct AnthropicMessageRequestContext {
    pub(super) identity_id: String,
    pub(super) endpoint_name: String,
    pub(super) model_requested: String,
    pub(super) is_streaming: bool,
    pub(super) started_at: std::time::Instant,
}

pub(super) async fn create_message_with_candidate(
    state: &AppState,
    access: &CurrentProtocolAccess,
    candidate_request: AnthropicMessageCandidateRequest,
    resolved: ResolvedEndpointProvider,
) -> Result<Response, AppError> {
    let AnthropicMessageCandidateRequest {
        original_payload,
        request,
        model_requested,
        is_streaming,
        started_at,
    } = candidate_request;
    let provider = resolved.provider;
    let upstream_protocol = resolved.upstream_protocol;
    let model_upstream = resolved.model_upstream;
    let mut upstream_payload = original_payload;
    upstream_payload["model"] = Value::String(model_upstream.clone());
    let mut request = request;
    request.model = model_upstream;
    let context = AnthropicMessageRequestContext {
        identity_id: access.identity_id.clone(),
        endpoint_name: access.endpoint_name.clone(),
        model_requested,
        is_streaming,
        started_at,
    };
    if upstream_protocol == UpstreamProtocol::AnthropicMessages {
        return create_native_anthropic_message(state, upstream_payload, provider, context)
            .await
            .map(IntoResponse::into_response);
    }
    if upstream_protocol == UpstreamProtocol::Responses {
        return create_responses_anthropic_message(state, request, provider, context)
            .await
            .map(IntoResponse::into_response);
    }

    create_chat_anthropic_message(state, request, provider, upstream_protocol, context).await
}
