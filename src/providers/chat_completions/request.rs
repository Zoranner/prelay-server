use serde_json::{json, Value};

use crate::bridge::internal::{
    InternalContentPart, InternalMessage, InternalRequest, InternalRole, InternalTool,
    InternalToolCall,
};

#[cfg(test)]
use crate::error::AppError;

#[cfg(test)]
pub fn decode_chat_request(value: Value) -> Result<InternalRequest, AppError> {
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
    let reasoning_requested =
        value.get("reasoning").is_some() || value.get("reasoning_effort").is_some();
    let tool_choice_requested = value.get("tool_choice").is_some();
    let structured_output_requested = value
        .pointer("/response_format/type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "text");
    let parallel_tool_calls_requested = value
        .get("parallel_tool_calls")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let streaming_usage_requested = value
        .pointer("/stream_options/include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("messages 必须是数组".to_string()))?
        .iter()
        .map(decode_message)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(InternalRequest {
        model,
        stream,
        max_tokens,
        max_completion_tokens: None,
        previous_response_id: None,
        instructions: None,
        store: true,
        reasoning: None,
        tool_choice: None,
        parallel_tool_calls: None,
        text: None,
        reasoning_requested,
        tool_choice_requested,
        structured_output_requested,
        parallel_tool_calls_requested,
        streaming_usage_requested,
        tools: decode_tools(value.get("tools"))?,
        messages,
    })
}

pub fn encode_chat_request(request: &InternalRequest) -> Value {
    let mut value = json!({
        "model": request.model,
        "stream": request.stream,
        "messages": request.messages.iter().map(encode_message).collect::<Vec<_>>(),
    });
    if let Some(max_completion_tokens) = request.max_completion_tokens {
        value["max_completion_tokens"] = json!(max_completion_tokens);
    } else if let Some(max_tokens) = request.max_tokens {
        value["max_tokens"] = json!(max_tokens);
    }
    if let Some(tool_choice) = &request.tool_choice {
        value["tool_choice"] = normalize_tool_choice(tool_choice);
    }
    if let Some(parallel_tool_calls) = request.parallel_tool_calls {
        value["parallel_tool_calls"] = json!(parallel_tool_calls);
    }
    if !request.tools.is_empty() {
        value["tools"] = json!(request.tools.iter().map(encode_tool).collect::<Vec<_>>());
    }
    value
}

fn normalize_tool_choice(value: &Value) -> Value {
    if value.get("type").and_then(Value::as_str) == Some("function")
        && value.get("name").and_then(Value::as_str).is_some()
        && value.get("function").is_none()
    {
        return json!({
            "type": "function",
            "function": { "name": value["name"] }
        });
    }
    value.clone()
}

#[cfg(test)]
fn decode_message(value: &Value) -> Result<InternalMessage, AppError> {
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .map(decode_role)
        .unwrap_or(InternalRole::User);
    let content = value
        .get("content")
        .map(decode_content)
        .transpose()?
        .unwrap_or_default();
    let tool_calls = value
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|tool_calls| {
            tool_calls
                .iter()
                .filter_map(decode_internal_tool_call)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(InternalMessage {
        role,
        content,
        tool_call_id: value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_calls,
        reasoning_content: value
            .get("reasoning_content")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

#[cfg(test)]
fn decode_content(value: &Value) -> Result<Vec<InternalContentPart>, AppError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
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

#[cfg(test)]
fn decode_tools(value: Option<&Value>) -> Result<Vec<InternalTool>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let tools = value
        .as_array()
        .ok_or_else(|| AppError::BadRequest("tools 必须是数组".to_string()))?;
    tools
        .iter()
        .map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| AppError::BadRequest("tool.function.name 不能为空".to_string()))?
                .to_string();
            let description = function
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string);
            let input_schema = function
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object" }));

            Ok(InternalTool {
                name,
                description,
                input_schema,
            })
        })
        .collect()
}

#[cfg(test)]
fn decode_internal_tool_call(value: &Value) -> Option<InternalToolCall> {
    let id = value.get("id").and_then(Value::as_str)?.to_string();
    let function = value.get("function")?;
    let name = function.get("name").and_then(Value::as_str)?.to_string();
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_string();

    Some(InternalToolCall {
        id,
        name,
        arguments,
    })
}

fn encode_message(message: &InternalMessage) -> Value {
    let mut value = json!({
        "role": encode_role(&message.role),
        "content": join_text_content(&message.content),
    });
    if matches!(message.role, InternalRole::Tool) {
        if let Some(tool_call_id) = &message.tool_call_id {
            value["tool_call_id"] = json!(tool_call_id);
        }
    }
    if !message.tool_calls.is_empty() {
        value["tool_calls"] = json!(message
            .tool_calls
            .iter()
            .map(encode_tool_call)
            .collect::<Vec<_>>());
        if matches!(message.role, InternalRole::Assistant) {
            if let Some(reasoning_content) = &message.reasoning_content {
                value["reasoning_content"] = json!(reasoning_content);
            }
        }
    }
    value
}

fn encode_tool_call(tool_call: &InternalToolCall) -> Value {
    json!({
        "id": tool_call.id,
        "type": "function",
        "function": {
            "name": tool_call.name,
            "arguments": tool_call.arguments,
        }
    })
}

fn encode_tool(tool: &InternalTool) -> Value {
    let mut function = json!({
        "name": tool.name,
        "parameters": tool.input_schema,
    });
    if let Some(description) = &tool.description {
        function["description"] = json!(description);
    }
    json!({
        "type": "function",
        "function": function,
    })
}

fn encode_role(role: &InternalRole) -> &'static str {
    match role {
        InternalRole::User => "user",
        InternalRole::Assistant => "assistant",
        InternalRole::System => "system",
        InternalRole::Tool => "tool",
    }
}

#[cfg(test)]
fn decode_role(role: &str) -> InternalRole {
    match role {
        "assistant" => InternalRole::Assistant,
        "system" | "developer" => InternalRole::System,
        "tool" => InternalRole::Tool,
        _ => InternalRole::User,
    }
}

#[cfg(test)]
fn decode_content_part_text(part: &Value) -> Option<String> {
    part.get("text")
        .or_else(|| part.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
fn value_to_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn join_text_content(content: &[InternalContentPart]) -> String {
    content
        .iter()
        .map(|part| match part {
            InternalContentPart::Text(text) => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
