use axum::response::Response;

use crate::{
    bridge::{internal::InternalRequest, responses::decode::validate_responses_bridge_payload},
    error::AppError,
    providers::{responses::encode_responses_request, spec::UpstreamProtocol},
    routes::v1::{auth::CurrentProtocolAccess, endpoint_resolver::ResolvedEndpointProvider},
    AppState,
};

use super::{
    chat::create_chat_response, handler::ResponseCandidateRequest, native::create_native_response,
    sessions::request_with_session_history,
};

pub(super) struct ResponseBridgeContext {
    pub(super) identity_id: String,
    pub(super) endpoint_name: String,
    pub(super) model_requested: String,
    pub(super) is_streaming: bool,
    pub(super) conversation_id: String,
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
        model_requested,
        is_streaming,
        previous_response_id,
        conversation_id,
        started_at,
    } = candidate_request;
    let provider = resolved.provider;
    let upstream_protocol = resolved.upstream_protocol;
    let model_upstream = resolved.model_upstream;
    let context = ResponseBridgeContext {
        identity_id: access.identity_id.clone(),
        endpoint_name: access.endpoint_name.clone(),
        model_requested,
        is_streaming,
        conversation_id,
        started_at,
    };

    if upstream_protocol == UpstreamProtocol::Responses {
        if previous_response_id.is_some() {
            let request = request_with_session_history(
                &state.storage,
                &access.identity_id,
                request_for_upstream(request.as_ref(), &model_upstream),
            )
            .await?;
            let payload = encode_responses_request(&request);
            return create_native_response(
                state,
                &payload,
                &request,
                &model_upstream,
                provider,
                previous_response_id,
                context,
            )
            .await;
        }
        return create_native_response(
            state,
            payload.as_ref(),
            request.as_ref(),
            &model_upstream,
            provider,
            previous_response_id,
            context,
        )
        .await;
    }

    validate_responses_bridge_payload(payload.as_ref())?;
    let request = request_for_upstream(request.as_ref(), &model_upstream);
    create_chat_response(
        state,
        request,
        provider,
        upstream_protocol,
        previous_response_id,
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
