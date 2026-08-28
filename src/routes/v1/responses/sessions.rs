use crate::{bridge::internal::InternalRequest, error::AppError, storage::Storage};

pub(super) async fn request_with_session_history(
    storage: &Storage,
    identity_id: &str,
    mut request: InternalRequest,
) -> Result<InternalRequest, AppError> {
    let Some(previous_response_id) = request.previous_response_id.as_deref() else {
        return Ok(request);
    };
    let Some(mut history) = storage
        .load_response_session_messages(identity_id, previous_response_id)
        .await?
    else {
        return Err(AppError::BadRequest(format!(
            "previous_response_id {previous_response_id} 不存在"
        )));
    };
    history.extend(request.messages);
    request.messages = history;
    Ok(request)
}

pub(super) fn count_tool_calls(response: &crate::bridge::internal::InternalResponse) -> i64 {
    response
        .output
        .iter()
        .filter(|item| item.is_tool_call())
        .count() as i64
}
