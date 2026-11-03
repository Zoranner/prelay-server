use axum::response::Response;

use crate::{
    codec::providers::spec::UpstreamProtocol,
    codec::{
        bridge::responses::decode::{
            decode_items_for_chat_bridge, validate_responses_bridge_payload,
        },
        internal::InternalRequest,
    },
    domain::activity::rollout::SessionContext,
    error::AppError,
    inbound::v1::{auth::CurrentProtocolAccess, endpoint_resolver::ResolvedEndpointProvider},
    AppState,
};

use super::{
    chat::create_chat_response,
    handler::ResponseCandidateRequest,
    native::{create_native_response, NativeResponseRequest},
    state::{continuation_items, current_input_items},
};

pub(super) struct ResponseBridgeContext {
    pub(super) identity_id: String,
    pub(super) endpoint_name: String,
    pub(super) response_id: String,
    pub(super) model_requested: String,
    pub(super) is_streaming: bool,
    pub(super) conversation_id: String,
    pub(super) session_id: String,
    pub(super) session: SessionContext,
    pub(super) started_at: std::time::Instant,
}

pub(super) async fn create_response_with_candidate(
    state: &AppState,
    access: &CurrentProtocolAccess,
    candidate_request: ResponseCandidateRequest,
    resolved: ResolvedEndpointProvider,
) -> Result<Response, AppError> {
    let ResponseCandidateRequest {
        payload,
        request,
        parent_state,
        response_id,
        model_requested,
        is_streaming,
        previous_response_id,
        conversation_id,
        started_at,
    } = candidate_request;
    let provider = resolved.provider;
    let upstream_protocol = resolved.upstream_protocol;
    let model_upstream = resolved.model_upstream;
    let session_id = access
        .session_id
        .clone()
        .unwrap_or_else(|| conversation_id.clone());
    let session = access.session_context(&conversation_id);
    let context = ResponseBridgeContext {
        identity_id: access.identity_id.clone(),
        endpoint_name: access.endpoint_name.clone(),
        response_id,
        model_requested,
        is_streaming,
        conversation_id,
        session_id,
        session,
        started_at,
    };
    let current_items = current_input_items(payload.as_ref())?;
    let input_items = match parent_state.as_deref() {
        Some(parent) => continuation_items(parent, &current_items),
        None => current_items,
    };

    if upstream_protocol == UpstreamProtocol::Responses {
        return create_native_response(
            state,
            NativeResponseRequest {
                payload: payload.as_ref(),
                request: request.as_ref(),
                previous_response_id,
                input_items: &input_items,
            },
            &model_upstream,
            provider,
            context,
        )
        .await;
    }

    validate_responses_bridge_payload(payload.as_ref())?;
    let mut request = request_for_upstream(request.as_ref(), &model_upstream);
    request.messages = decode_items_for_chat_bridge(&input_items)?;
    create_chat_response(
        state,
        request,
        provider,
        upstream_protocol,
        previous_response_id,
        &input_items,
        context,
    )
    .await
}

/// 桥接与历史会话路径需要请求带上游模型名；原生直通路径不拷贝请求。
fn request_for_upstream(request: &InternalRequest, model_upstream: &str) -> InternalRequest {
    let mut request = request.clone();
    request.model = model_upstream.to_string();
    request
}
