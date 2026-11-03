use crate::codec::internal::InternalResponse;

pub(super) fn count_tool_calls(response: &InternalResponse) -> i64 {
    response
        .output
        .iter()
        .filter(|item| item.is_tool_call())
        .count() as i64
}
