use std::borrow::Cow;

use serde_json::{json, Value};

use crate::{
    codec::activity::conversation_events_from_responses_request,
    domain::activity::ContentDraft,
    error::AppError,
    inbound::v1::responses::image_budget::{self, ImageBudgetOutcome, ImageBudgetPolicy},
};

pub(super) fn prepare_native_input<'a>(
    payload: &Value,
    input_items: &'a [Value],
    policy: &ImageBudgetPolicy,
) -> Result<(Cow<'a, [Value]>, Vec<ContentDraft>), AppError> {
    let budgeted = budget_input_items(input_items, policy)?;
    let events = conversation_events_from_responses_request(payload, budgeted.as_ref());
    Ok((budgeted, events))
}

pub(super) fn budget_input_items<'a>(
    input_items: &'a [Value],
    policy: &ImageBudgetPolicy,
) -> Result<Cow<'a, [Value]>, AppError> {
    if !policy.enabled {
        return Ok(Cow::Borrowed(input_items));
    }
    let budget_payload = json!({ "input": input_items });
    match image_budget::apply(&budget_payload, policy) {
        ImageBudgetOutcome::Disabled => Ok(Cow::Borrowed(input_items)),
        ImageBudgetOutcome::WithinBudget(report) => {
            tracing::debug!(
                total_bytes = report.total_bytes,
                max_bytes = policy.max_bytes,
                "image payload is within the upstream budget"
            );
            Ok(Cow::Borrowed(input_items))
        }
        ImageBudgetOutcome::Trimmed { payload, report } => {
            tracing::info!(
                total_bytes = report.total_bytes,
                remaining_bytes = report.remaining_bytes,
                dropped_images = report.dropped_images,
                max_bytes = policy.max_bytes,
                "trimmed historical images for the upstream image budget"
            );
            Ok(Cow::Owned(
                payload
                    .get("input")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            ))
        }
        ImageBudgetOutcome::CannotMeetBudget(report) => {
            tracing::warn!(
                total_bytes = report.total_bytes,
                remaining_bytes = report.remaining_bytes,
                dropped_images = report.dropped_images,
                max_bytes = policy.max_bytes,
                keep_last = policy.keep_last,
                "image payload cannot be reduced to the upstream budget"
            );
            Err(AppError::PayloadTooLarge(format!(
                "图片请求体超过上游预算：已丢弃 {} 张历史图片，仍有 {} 字节，预算 {} 字节。请减少图片，或调整 IMAGE_BUDGET_MAX_BYTES。",
                report.dropped_images, report.remaining_bytes, policy.max_bytes
            )))
        }
    }
}
