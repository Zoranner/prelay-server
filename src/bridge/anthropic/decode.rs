use serde_json::Value;

use crate::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalRequest, InternalRole, InternalTool,
    },
    error::AppError,
};

pub fn decode_anthropic_request(value: Value) -> Result<InternalRequest, AppError> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("model 不能为空".to_string()))?
        .to_string();
    let stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_tokens = value.get("max_tokens").and_then(Value::as_i64);
    let reasoning_requested = value.get("thinking").is_some() || value.get("reasoning").is_some();
    let tool_choice_requested = value.get("tool_choice").is_some();
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("messages 不能为空".to_string()))?;
    let mut decoded = Vec::new();
    if let Some(system) = value.get("system") {
        decoded.push(InternalMessage {
            role: InternalRole::System,
            content: decode_content(system)?,
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        });
    }
    decoded.extend(
        messages
            .iter()
            .map(decode_message)
            .collect::<Result<Vec<_>, _>>()?,
    );

    let request = InternalRequest {
        model,
        stream,
        max_tokens,
        previous_response_id: None,
        reasoning_requested,
        tool_choice_requested,
        structured_output_requested: false,
        parallel_tool_calls_requested: false,
        streaming_usage_requested: false,
        tools: decode_tools(value.get("tools"))?,
        messages: decoded,
    };
    Ok(request)
}

fn decode_tools(value: Option<&Value>) -> Result<Vec<InternalTool>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(tools) = value.as_array() else {
        return Ok(Vec::new());
    };

    Ok(tools.iter().filter_map(decode_tool).collect())
}

fn decode_tool(tool: &Value) -> Option<InternalTool> {
    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())?;
    let name = name.to_string();
    let input_schema = tool
        .get("input_schema")
        .or_else(|| tool.get("parameters"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "type": "object" }));

    Some(InternalTool {
        name,
        description: tool
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        input_schema,
    })
}

fn decode_message(value: &Value) -> Result<InternalMessage, AppError> {
    if let Some(tool_message) = decode_tool_result_message(value)? {
        return Ok(tool_message);
    }

    let role = value
        .get("role")
        .and_then(Value::as_str)
        .map(decode_role)
        .unwrap_or(InternalRole::User);
    let content = value.get("content").unwrap_or(value);

    Ok(InternalMessage {
        role,
        content: decode_content(content)?,
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    })
}

fn decode_tool_result_message(value: &Value) -> Result<Option<InternalMessage>, AppError> {
    let Some(parts) = value.get("content").and_then(Value::as_array) else {
        return Ok(None);
    };
    let Some(tool_result) = parts.iter().find(|part| {
        part.get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "tool_result")
    }) else {
        return Ok(None);
    };
    let Some(tool_call_id) = tool_result
        .get("tool_use_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
    else {
        return Ok(None);
    };

    Ok(Some(InternalMessage {
        role: InternalRole::Tool,
        content: decode_tool_result_content(tool_result.get("content"))?,
        tool_call_id: Some(tool_call_id.to_string()),
        tool_calls: Vec::new(),
        reasoning_content: None,
    }))
}

fn decode_tool_result_content(value: Option<&Value>) -> Result<Vec<InternalContentPart>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if let Some(text) = value.as_str() {
        return Ok(vec![InternalContentPart::Text(text.to_string())]);
    }
    decode_content(value)
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
        return Ok(vec![InternalContentPart::Text(value_to_text(value))]);
    };
    let text_parts = parts
        .iter()
        .filter_map(|part| decode_content_part_text(part).map(InternalContentPart::Text))
        .collect::<Vec<_>>();

    if text_parts.is_empty() {
        return Ok(parts
            .iter()
            .map(|part| InternalContentPart::Text(value_to_text(part)))
            .collect());
    }

    Ok(text_parts)
}

fn decode_content_part_text(part: &Value) -> Option<String> {
    let kind = part.get("type").and_then(Value::as_str);
    if kind.is_some_and(|kind| kind == "tool_result" || kind == "tool_use") {
        if kind.is_some_and(|kind| kind == "tool_result") && part.get("tool_use_id").is_none() {
            return part
                .get("content")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        return None;
    }

    part.get("text")
        .or_else(|| part.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn value_to_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
