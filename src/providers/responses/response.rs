use serde_json::Value;

use crate::{
    bridge::internal::{InternalContentPart, InternalOutputItem, InternalResponse, InternalRole},
    error::AppError,
};

pub fn decode_responses_response(value: Value) -> Result<InternalResponse, AppError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_unknown")
        .to_string();
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::UpstreamInvalidResponse {
            message: "上游响应格式无效".to_string(),
        })?
        .iter()
        .filter_map(decode_output_item)
        .collect::<Vec<_>>();

    Ok(InternalResponse {
        id,
        model,
        output,
        usage: crate::bridge::usage::decode_usage(value.get("usage")),
    })
}

fn decode_output_item(item: &Value) -> Option<InternalOutputItem> {
    match item.get("type").and_then(Value::as_str)? {
        "message" => Some(InternalOutputItem::Message {
            id: item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("msg_0")
                .to_string(),
            role: item
                .get("role")
                .and_then(Value::as_str)
                .map(decode_role)
                .unwrap_or(InternalRole::Assistant),
            content: decode_message_content(item.get("content"))?,
        }),
        "function_call" => Some(InternalOutputItem::FunctionToolCall {
            id: item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)?
                .to_string(),
            name: item.get("name").and_then(Value::as_str)?.to_string(),
            arguments: item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string(),
            reasoning_content: None,
        }),
        _ => None,
    }
}

fn decode_message_content(content: Option<&Value>) -> Option<Vec<InternalContentPart>> {
    let parts = content?.as_array()?;
    let text = parts
        .iter()
        .filter(|part| {
            part.get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "output_text")
        })
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    Some(vec![InternalContentPart::Text(text)])
}

fn decode_role(role: &str) -> InternalRole {
    match role {
        "user" => InternalRole::User,
        "system" | "developer" => InternalRole::System,
        "tool" => InternalRole::Tool,
        _ => InternalRole::Assistant,
    }
}
