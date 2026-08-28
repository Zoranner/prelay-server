use axum::response::Response;
use serde_json::Value;

use crate::{
    error::AppError,
    observability::request_metadata::build_request_metadata,
    providers::{responses::encode_responses_request, spec::UpstreamProtocol},
    routes::v1::{auth::CurrentProtocolAccess, endpoint_resolver::ResolvedEndpointProvider},
    AppState,
};

use super::{
    anthropic::create_anthropic_messages_response, chat::create_chat_response,
    handler::ResponseCandidateRequest, native::create_native_response,
    sessions::request_with_session_history,
};

pub(super) struct ResponseBridgeContext {
    pub(super) identity_id: String,
    pub(super) endpoint_name: String,
    pub(super) model_requested: String,
    pub(super) is_streaming: bool,
    pub(super) metadata_json: Option<String>,
    pub(super) started_at: std::time::Instant,
}

pub(super) async fn create_response_with_candidate(
    state: &AppState,
    access: &CurrentProtocolAccess,
    candidate_request: ResponseCandidateRequest,
    resolved: ResolvedEndpointProvider,
) -> Result<Response, AppError> {
    let ResponseCandidateRequest {
        original_payload,
        request,
        diagnostics,
        model_requested,
        is_streaming,
        previous_response_id,
        started_at,
    } = candidate_request;
    let provider = resolved.provider;
    let upstream_protocol = resolved.upstream_protocol;
    let model_upstream = resolved.model_upstream;
    let metadata_json = build_request_metadata(diagnostics)?;
    let mut upstream_payload = original_payload;
    upstream_payload["model"] = Value::String(model_upstream.clone());
    let mut request = request;
    request.model = model_upstream;
    if upstream_protocol == UpstreamProtocol::Responses {
        let (upstream_payload, request) = if previous_response_id.is_some() {
            let request =
                request_with_session_history(&state.storage, &access.identity_id, request).await?;
            (encode_responses_request(&request), request)
        } else {
            (upstream_payload, request)
        };
        return create_native_response(
            state,
            upstream_payload,
            provider,
            request,
            previous_response_id,
            ResponseBridgeContext {
                identity_id: access.identity_id.clone(),
                endpoint_name: access.endpoint_name.clone(),
                model_requested,
                is_streaming,
                metadata_json,
                started_at,
            },
        )
        .await;
    }
    if upstream_protocol == UpstreamProtocol::AnthropicMessages {
        return create_anthropic_messages_response(
            state,
            request,
            provider,
            previous_response_id,
            ResponseBridgeContext {
                identity_id: access.identity_id.clone(),
                endpoint_name: access.endpoint_name.clone(),
                model_requested,
                is_streaming,
                metadata_json,
                started_at,
            },
        )
        .await;
    }

    create_chat_response(
        state,
        request,
        provider,
        upstream_protocol,
        previous_response_id,
        ResponseBridgeContext {
            identity_id: access.identity_id.clone(),
            endpoint_name: access.endpoint_name.clone(),
            model_requested,
            is_streaming,
            metadata_json,
            started_at,
        },
    )
    .await
}
