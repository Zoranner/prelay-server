use serde_json::Value;

use crate::{
    codec::internal::{
        InternalContentPart, InternalImagePart, InternalMessage, InternalRole, InternalToolCall,
    },
    error::AppError,
};

pub fn decode_items_for_chat_bridge(items: &[Value]) -> Result<Vec<InternalMessage>, AppError> {
    items.iter().map(decode_item).collect()
}

fn decode_item(value: &Value) -> Result<InternalMessage, AppError> {
    match value.get("type").and_then(Value::as_str) {
        Some("function_call") => decode_function_call(value),
        Some("function_call_output") => decode_function_call_output(value),
        Some("message") | None => decode_message(value),
        Some(item_type) => Err(AppError::openai_unsupported(
            "input",
            format!("Responses input item type `{item_type}` cannot be bridged"),
        )),
    }
}

fn decode_message(value: &Value) -> Result<InternalMessage, AppError> {
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("message item requires a role"))?;
    let role = match role {
        "user" => InternalRole::User,
        "assistant" => InternalRole::Assistant,
        "system" | "developer" => InternalRole::System,
        other => return Err(invalid(format!("message role `{other}` cannot be bridged"))),
    };
    let content = value
        .get("content")
        .ok_or_else(|| invalid("message item requires content"))?;

    Ok(InternalMessage {
        role,
        content: decode_content(content)?,
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    })
}

fn decode_function_call(value: &Value) -> Result<InternalMessage, AppError> {
    let call_id = string_field(value.get("call_id").or_else(|| value.get("id")))
        .ok_or_else(|| invalid("function_call item requires a non-empty call_id"))?;
    let name = string_field(value.get("name"))
        .ok_or_else(|| invalid("function_call item requires a non-empty name"))?;
    let arguments = value
        .get("arguments")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("function_call item requires string arguments"))?;

    Ok(InternalMessage {
        role: InternalRole::Assistant,
        content: Vec::new(),
        tool_call_id: None,
        tool_calls: vec![InternalToolCall {
            id: call_id,
            name,
            arguments: arguments.to_string(),
        }],
        reasoning_content: None,
    })
}

fn decode_function_call_output(value: &Value) -> Result<InternalMessage, AppError> {
    let call_id = string_field(value.get("call_id"))
        .ok_or_else(|| invalid("function_call_output item requires a non-empty call_id"))?;
    let output = value
        .get("output")
        .ok_or_else(|| invalid("function_call_output item requires output"))?;

    Ok(InternalMessage {
        role: InternalRole::Tool,
        content: decode_content(output)?,
        tool_call_id: Some(call_id),
        tool_calls: Vec::new(),
        reasoning_content: None,
    })
}

fn decode_content(value: &Value) -> Result<Vec<InternalContentPart>, AppError> {
    if let Some(text) = value.as_str() {
        return Ok(vec![InternalContentPart::Text(text.to_string())]);
    }
    let parts = value
        .as_array()
        .ok_or_else(|| invalid("content must be a string or an array of content parts"))?;
    parts.iter().map(decode_content_part).collect()
}

fn decode_content_part(part: &Value) -> Result<InternalContentPart, AppError> {
    let part_type = part
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("content part requires a type"))?;
    match part_type {
        "input_text" | "output_text" | "text" => part
            .get("text")
            .and_then(Value::as_str)
            .map(|text| InternalContentPart::Text(text.to_string()))
            .ok_or_else(|| invalid(format!("{part_type} content part requires text"))),
        "refusal" => part
            .get("refusal")
            .and_then(Value::as_str)
            .map(|text| InternalContentPart::Text(text.to_string()))
            .ok_or_else(|| invalid("refusal content part requires refusal")),
        "input_image" => {
            let image_url = part.get("image_url").cloned();
            let file_id = string_field(part.get("file_id"));
            if image_url.is_none() && file_id.is_none() {
                return Err(invalid(
                    "input_image content part requires image_url or file_id",
                ));
            }
            Ok(InternalContentPart::Image(InternalImagePart {
                image_url,
                file_id,
                detail: string_field(part.get("detail")),
            }))
        }
        other => Err(invalid(format!(
            "content part type `{other}` cannot be bridged"
        ))),
    }
}

fn string_field(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::openai_invalid("input", message)
}
