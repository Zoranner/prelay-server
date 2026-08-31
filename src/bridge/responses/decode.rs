use serde_json::Value;

use crate::bridge::internal::{
    InternalContentPart, InternalMessage, InternalRequest, InternalRole, InternalTool,
};
use crate::error::AppError;

pub fn decode_responses_request(value: Value) -> Result<InternalRequest, AppError> {
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
    let previous_response_id = value
        .get("previous_response_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string);
    let max_tokens = value.get("max_output_tokens").and_then(Value::as_i64);
    let reasoning_requested = value.get("reasoning").is_some();
    let tool_choice_requested = value.get("tool_choice").is_some();
    let structured_output_requested = responses_structured_output_requested(&value);
    let parallel_tool_calls_requested = value
        .get("parallel_tool_calls")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let streaming_usage_requested = value
        .pointer("/stream_options/include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let input = value
        .get("input")
        .ok_or_else(|| AppError::BadRequest("input 不能为空".to_string()))?;

    let request = InternalRequest {
        model,
        stream,
        max_tokens,
        previous_response_id,
        reasoning_requested,
        tool_choice_requested,
        structured_output_requested,
        parallel_tool_calls_requested,
        streaming_usage_requested,
        tools: decode_tools(value.get("tools"))?,
        messages: decode_input(input)?,
    };
    Ok(request)
}

fn responses_structured_output_requested(value: &Value) -> bool {
    value
        .pointer("/text/format/type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "text")
        || value
            .pointer("/response_format/type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "text")
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
    let function = tool.get("function").unwrap_or(tool);
    let name = function
        .get("name")
        .or_else(|| tool.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())?;
    let name = name.to_string();
    let description = function
        .get("description")
        .or_else(|| tool.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let input_schema = function
        .get("parameters")
        .or_else(|| function.get("input_schema"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "type": "object" }));

    Some(InternalTool {
        name,
        description,
        input_schema,
    })
}

fn decode_input(input: &Value) -> Result<Vec<InternalMessage>, AppError> {
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
    items.iter().map(decode_message).collect()
}

fn decode_message(value: &Value) -> Result<InternalMessage, AppError> {
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "function_call_output")
    {
        return Ok(InternalMessage {
            role: InternalRole::Tool,
            content: vec![InternalContentPart::Text(
                value
                    .get("output")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            )],
            tool_call_id: value
                .get("call_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            tool_calls: Vec::new(),
            reasoning_content: None,
        });
    }

    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "function_call")
    {
        return Ok(InternalMessage {
            role: InternalRole::Assistant,
            content: Vec::new(),
            tool_call_id: None,
            tool_calls: decode_function_call(value).into_iter().collect(),
            reasoning_content: None,
        });
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
        tool_call_id: value
            .get("call_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_calls: Vec::new(),
        reasoning_content: None,
    })
}

fn decode_function_call(value: &Value) -> Option<crate::bridge::internal::InternalToolCall> {
    let id = value
        .get("call_id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "call_unknown".to_string());
    let name = value.get("name").and_then(Value::as_str)?.to_string();
    let arguments = match value.get("arguments") {
        Some(arguments) if arguments.is_string() => value_to_text(arguments),
        Some(arguments) => value_to_text(arguments),
        None => "{}".to_string(),
    };

    Some(crate::bridge::internal::InternalToolCall {
        id,
        name,
        arguments,
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
