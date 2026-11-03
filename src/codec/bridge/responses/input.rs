use serde_json::Value;

use crate::{
    codec::internal::{
        InternalContentPart, InternalImagePart, InternalMessage, InternalRole, InternalToolCall,
    },
    error::AppError,
};

pub(super) fn decode_input(input: &Value) -> Result<Vec<InternalMessage>, AppError> {
    if input.is_null() {
        return Ok(Vec::new());
    }
    if let Some(text) = input.as_str() {
        return Ok(vec![InternalMessage {
            role: InternalRole::User,
            content: vec![InternalContentPart::Text(text.to_string())],
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        }]);
    }

    let items = input
        .as_array()
        .ok_or_else(|| AppError::BadRequest("input 必须是字符串或消息数组".to_string()))?;
    let mut messages = Vec::new();
    for item in items {
        if let Some(message) = decode_item(item)? {
            messages.push(message);
        }
    }
    Ok(messages)
}

fn decode_item(value: &Value) -> Result<Option<InternalMessage>, AppError> {
    match value.get("type").and_then(Value::as_str) {
        Some("function_call_output") => Ok(Some(decode_function_call_output(value)?)),
        Some("function_call") => Ok(Some(decode_function_call(value)?)),
        Some("reasoning") => Ok(decode_reasoning(value)),
        Some("message") | None => Ok(Some(decode_message(value)?)),
        // Native Responses passes the original item through unchanged. These
        // items have no faithful InternalMessage representation and must not
        // be coerced into user text.
        Some("configuration_update" | "item_reference") => Ok(None),
        Some(_) => Ok(None),
    }
}

fn decode_message(value: &Value) -> Result<InternalMessage, AppError> {
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("message item requires a role"))?;
    let role = decode_role(role);
    let content = value
        .get("content")
        .ok_or_else(|| invalid("message item requires content"))?;

    Ok(InternalMessage {
        role,
        content: decode_content(content)?,
        tool_call_id: value
            .get("call_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_calls: Vec::new(),
        reasoning_content: None,
    })
}

fn decode_reasoning(value: &Value) -> Option<InternalMessage> {
    let summary = reasoning_text(value.get("summary"), "summary_text");
    let content = reasoning_text(value.get("content"), "reasoning_text");
    let reasoning = if summary.is_empty() { content } else { summary };
    if reasoning.is_empty() {
        return None;
    }
    Some(InternalMessage {
        role: InternalRole::Assistant,
        content: Vec::new(),
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: Some(reasoning),
    })
}

fn reasoning_text(value: Option<&Value>, expected_type: &str) -> String {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| {
            part.get("type")
                .and_then(Value::as_str)
                .is_none_or(|kind| kind == expected_type)
        })
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_function_call_output(value: &Value) -> Result<InternalMessage, AppError> {
    let output = value.get("output").cloned().unwrap_or(Value::Null);
    let content = if output.is_null() {
        Vec::new()
    } else {
        decode_content(&output)?
    };
    Ok(InternalMessage {
        role: InternalRole::Tool,
        content,
        tool_call_id: value
            .get("call_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_calls: Vec::new(),
        reasoning_content: None,
    })
}

fn decode_function_call(value: &Value) -> Result<InternalMessage, AppError> {
    let id = value
        .get("call_id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "call_unknown".to_string());
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("function_call item requires a name"))?
        .to_string();
    let arguments = match value.get("arguments") {
        Some(arguments) if arguments.is_string() => value_to_text(arguments),
        Some(arguments) => value_to_text(arguments),
        None => "{}".to_string(),
    };

    Ok(InternalMessage {
        role: InternalRole::Assistant,
        content: Vec::new(),
        tool_call_id: None,
        tool_calls: vec![InternalToolCall {
            id,
            name,
            arguments,
        }],
        reasoning_content: None,
    })
}

fn decode_role(role: &str) -> InternalRole {
    match role {
        "user" => InternalRole::User,
        "assistant" => InternalRole::Assistant,
        "system" | "developer" => InternalRole::System,
        "tool" => InternalRole::Tool,
        _ => InternalRole::User,
    }
}

fn decode_content(value: &Value) -> Result<Vec<InternalContentPart>, AppError> {
    if let Some(text) = value.as_str() {
        return Ok(vec![InternalContentPart::Text(text.to_string())]);
    }

    let Some(parts) = value.as_array() else {
        return Err(invalid(
            "content must be a string or an array of content parts",
        ));
    };
    Ok(parts.iter().filter_map(decode_content_part).collect())
}

fn decode_content_part(part: &Value) -> Option<InternalContentPart> {
    match part.get("type").and_then(Value::as_str) {
        Some("input_image") => {
            let image_url = part.get("image_url").cloned();
            let file_id = part
                .get("file_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            if image_url.is_none() && file_id.is_none() {
                return None;
            }
            Some(InternalContentPart::Image(InternalImagePart {
                image_url,
                file_id,
                detail: part
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }))
        }
        Some("input_text" | "text" | "output_text") => part
            .get("text")
            .or_else(|| part.get("content"))
            .and_then(Value::as_str)
            .map(|text| InternalContentPart::Text(text.to_string())),
        Some("refusal") => part
            .get("refusal")
            .and_then(Value::as_str)
            .map(|text| InternalContentPart::Text(text.to_string())),
        _ => None,
    }
}

fn value_to_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::openai_invalid("input", message)
}
