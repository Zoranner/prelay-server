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
    let input = value
        .get("input")
        .ok_or_else(|| AppError::BadRequest("input 不能为空".to_string()))?;

    Ok(InternalRequest {
        model,
        stream,
        max_tokens,
        previous_response_id,
        reasoning_requested,
        tool_choice_requested,
        structured_output_requested,
        tools: decode_tools(value.get("tools"))?,
        messages: decode_input(input)?,
    })
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
    let tools = value
        .as_array()
        .ok_or_else(|| AppError::BadRequest("tools 必须是数组".to_string()))?;

    tools
        .iter()
        .map(|tool| {
            let tool_type = tool
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::BadRequest("tool.type 不能为空".to_string()))?;
            if tool_type != "function" {
                return Err(AppError::BadRequest(format!(
                    "不支持的 Responses tool 类型: {tool_type}"
                )));
            }

            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| AppError::BadRequest("tool.name 不能为空".to_string()))?
                .to_string();
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string);
            let input_schema = tool
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "type": "object" }));

            Ok(InternalTool {
                name,
                description,
                input_schema,
            })
        })
        .collect()
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

    let role = value
        .get("role")
        .and_then(Value::as_str)
        .map(decode_role)
        .transpose()?
        .unwrap_or(InternalRole::User);
    let content = value
        .get("content")
        .ok_or_else(|| AppError::BadRequest("message.content 不能为空".to_string()))?;

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

fn decode_role(role: &str) -> Result<InternalRole, AppError> {
    match role {
        "user" => Ok(InternalRole::User),
        "assistant" => Ok(InternalRole::Assistant),
        "system" | "developer" => Ok(InternalRole::System),
        "tool" => Ok(InternalRole::Tool),
        _ => Err(AppError::BadRequest(format!("不支持的消息角色: {role}"))),
    }
}

fn decode_content(value: &Value) -> Result<Vec<InternalContentPart>, AppError> {
    if let Some(text) = value.as_str() {
        return Ok(vec![InternalContentPart::Text(text.to_string())]);
    }

    let parts = value
        .as_array()
        .ok_or_else(|| AppError::BadRequest("message.content 必须是字符串或数组".to_string()))?;
    let text_parts = parts
        .iter()
        .filter_map(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .map(|text| InternalContentPart::Text(text.to_string()))
        })
        .collect::<Vec<_>>();

    if text_parts.is_empty() {
        return Err(AppError::BadRequest(
            "message.content 没有可用文本".to_string(),
        ));
    }

    Ok(text_parts)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::decode_responses_request;
    use crate::bridge::internal::{InternalContentPart, InternalRole};

    #[test]
    fn decodes_string_input_into_user_message() {
        let request = decode_responses_request(json!({
            "model": "deepseek-chat",
            "input": "hello",
            "stream": true,
            "previous_response_id": "resp_previous"
        }))
        .expect("decode responses request");

        assert_eq!(request.model, "deepseek-chat");
        assert!(request.stream);
        assert_eq!(
            request.previous_response_id.as_deref(),
            Some("resp_previous")
        );
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, InternalRole::User);
        assert_eq!(
            request.messages[0].content,
            vec![InternalContentPart::Text("hello".to_string())]
        );
    }

    #[test]
    fn decodes_max_output_tokens_to_internal_max_tokens() {
        let request = decode_responses_request(json!({
            "model": "deepseek-chat",
            "input": "hello",
            "max_output_tokens": 1024
        }))
        .expect("decode responses request");

        assert_eq!(request.max_tokens, Some(1024));
    }

    #[test]
    fn decodes_message_array_input_into_internal_messages() {
        let request = decode_responses_request(json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "role": "system",
                    "content": "You are concise."
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "hello" },
                        { "type": "text", "text": "world" }
                    ]
                }
            ]
        }))
        .expect("decode responses request");

        assert!(!request.stream);
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, InternalRole::System);
        assert_eq!(
            request.messages[0].content,
            vec![InternalContentPart::Text("You are concise.".to_string())]
        );
        assert_eq!(request.messages[1].role, InternalRole::User);
        assert_eq!(
            request.messages[1].content,
            vec![
                InternalContentPart::Text("hello".to_string()),
                InternalContentPart::Text("world".to_string())
            ]
        );
    }

    #[test]
    fn decodes_function_tools_to_internal_tools() {
        let request = decode_responses_request(json!({
            "model": "deepseek-chat",
            "input": "hello",
            "tools": [
                {
                    "type": "function",
                    "name": "read_file",
                    "description": "Read a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"]
                    }
                }
            ]
        }))
        .expect("decode responses request");

        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name, "read_file");
        assert_eq!(request.tools[0].description.as_deref(), Some("Read a file"));
        assert_eq!(request.tools[0].input_schema["type"], "object");
        assert_eq!(
            request.tools[0].input_schema["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn decodes_function_call_output_into_tool_message() {
        let request = decode_responses_request(json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "{\"content\":\"file text\"}"
                }
            ]
        }))
        .expect("decode responses request");

        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, InternalRole::Tool);
        assert_eq!(request.messages[0].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(
            request.messages[0].content,
            vec![InternalContentPart::Text(
                "{\"content\":\"file text\"}".to_string()
            )]
        );
    }
}
