use serde_json::{json, Value};

use crate::{error::AppError, store::storage::ResponseState};

pub(super) fn current_input_items(payload: &Value) -> Result<Vec<Value>, AppError> {
    match payload.get("input") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(vec![json!({
            "role": "user",
            "content": text
        })]),
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(_) => Err(AppError::openai_invalid(
            "input",
            "input must be a string or an array of response items",
        )),
    }
}

pub(super) fn continuation_items(parent: &ResponseState, current: &[Value]) -> Vec<Value> {
    let mut items = parent.request_items.as_array().cloned().unwrap_or_default();
    items.extend(
        parent
            .response_items
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    items.extend(current.iter().cloned());
    items
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{continuation_items, current_input_items};
    use crate::store::storage::ResponseState;

    #[test]
    fn normalizes_string_input_into_a_user_item() {
        assert_eq!(
            current_input_items(&json!({ "input": "hello" })).expect("input"),
            vec![json!({ "role": "user", "content": "hello" })]
        );
    }

    #[test]
    fn keeps_item_arrays_and_rejects_other_input_types() {
        let items = json!([{ "type": "message", "role": "user", "content": "hello" }]);
        assert_eq!(
            current_input_items(&json!({ "input": items })).expect("input"),
            items.as_array().expect("items").clone()
        );
        assert!(current_input_items(&json!({ "input": { "text": "hello" } })).is_err());
    }

    #[test]
    fn appends_current_items_after_the_parent_turn() {
        let parent = ResponseState {
            identity_id: "identity".to_string(),
            response_id: "resp_parent".to_string(),
            previous_response_id: None,
            conversation_id: Some("conv_parent".to_string()),
            provider_id: "provider".to_string(),
            model: "model".to_string(),
            upstream_response_id: None,
            request_items: json!([{ "role": "user", "content": "first" }]),
            response_items: json!([{ "type": "message", "role": "assistant", "content": "first" }]),
            created_at: "2026-10-30T00:00:00Z".to_string(),
            expires_at: "2026-11-29T00:00:00Z".to_string(),
        };
        let current = json!([{ "role": "user", "content": "second" }]);

        assert_eq!(
            continuation_items(&parent, current.as_array().expect("current items")),
            vec![
                json!({ "role": "user", "content": "first" }),
                json!({ "type": "message", "role": "assistant", "content": "first" }),
                json!({ "role": "user", "content": "second" }),
            ]
        );
    }
}
