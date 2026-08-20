use serde_json::Value;

use crate::bridge::{
    diagnostics::{BridgeDiagnostic, DecodedRequest, DiagnosticAction, DiagnosticSeverity},
    internal::{InternalContentPart, InternalMessage, InternalRequest, InternalRole, InternalTool},
};
use crate::error::AppError;

#[cfg(test)]
pub fn decode_responses_request(value: Value) -> Result<InternalRequest, AppError> {
    Ok(decode_responses_request_with_diagnostics(value)?.request)
}

pub fn decode_responses_request_with_diagnostics(value: Value) -> Result<DecodedRequest, AppError> {
    let mut diagnostics = Vec::new();
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
        tools: decode_tools(value.get("tools"), &mut diagnostics)?,
        messages: decode_input(input, &mut diagnostics)?,
    };
    Ok(DecodedRequest {
        request,
        diagnostics,
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

fn decode_tools(
    value: Option<&Value>,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<Vec<InternalTool>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(tools) = value.as_array() else {
        return Ok(Vec::new());
    };

    Ok(tools
        .iter()
        .enumerate()
        .filter_map(|(index, tool)| decode_tool(tool, index, diagnostics))
        .collect())
}

fn decode_tool(
    tool: &Value,
    index: usize,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Option<InternalTool> {
    let function = tool.get("function").unwrap_or(tool);
    let Some(name) = function
        .get("name")
        .or_else(|| tool.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
    else {
        diagnostics.push(BridgeDiagnostic::new(
            "responses",
            format!("/tools/{index}"),
            DiagnosticAction::Ignored,
            DiagnosticSeverity::Warning,
            "responses.tool.unsupported",
            "跳过无法映射为 function tool 的工具定义",
            tool.get("type").and_then(Value::as_str).map(str::to_string),
        ));
        return None;
    };
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

fn decode_input(
    input: &Value,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<Vec<InternalMessage>, AppError> {
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
    items
        .iter()
        .enumerate()
        .map(|(index, value)| decode_message(value, &format!("/input/{index}"), diagnostics))
        .collect()
}

fn decode_message(
    value: &Value,
    path: &str,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<InternalMessage, AppError> {
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
            tool_calls: decode_function_call(value, path, diagnostics)
                .into_iter()
                .collect(),
            reasoning_content: None,
        });
    }

    let role = value
        .get("role")
        .and_then(Value::as_str)
        .map(|role| decode_role(role, &format!("{path}/role"), diagnostics))
        .unwrap_or(InternalRole::User);
    let content = value.get("content").unwrap_or(value);

    Ok(InternalMessage {
        role,
        content: decode_content(content, &format!("{path}/content"), diagnostics)?,
        tool_call_id: value
            .get("call_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_calls: Vec::new(),
        reasoning_content: None,
    })
}

fn decode_function_call(
    value: &Value,
    path: &str,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Option<crate::bridge::internal::InternalToolCall> {
    let id = value
        .get("call_id")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            diagnostics.push(BridgeDiagnostic::new(
                "responses",
                format!("{path}/call_id"),
                DiagnosticAction::Defaulted,
                DiagnosticSeverity::Warning,
                "responses.function_call.call_id_missing",
                "function_call 缺少 call_id，已补默认值",
                None,
            ));
            "call_unknown".to_string()
        });
    let name = value.get("name").and_then(Value::as_str)?.to_string();
    let arguments = match value.get("arguments") {
        Some(arguments) if arguments.is_string() => value_to_text(arguments),
        Some(arguments) => {
            diagnostics.push(BridgeDiagnostic::new(
                "responses",
                format!("{path}/arguments"),
                DiagnosticAction::Textified,
                DiagnosticSeverity::Info,
                "responses.function_call.arguments_non_string",
                "function_call arguments 不是字符串，已转为 JSON 字符串",
                value_kind(arguments),
            ));
            value_to_text(arguments)
        }
        None => "{}".to_string(),
    };

    Some(crate::bridge::internal::InternalToolCall {
        id,
        name,
        arguments,
    })
}

fn decode_role(role: &str, path: &str, diagnostics: &mut Vec<BridgeDiagnostic>) -> InternalRole {
    match role {
        "user" => InternalRole::User,
        "assistant" => InternalRole::Assistant,
        "system" | "developer" => InternalRole::System,
        "tool" => InternalRole::Tool,
        _ => {
            diagnostics.push(BridgeDiagnostic::new(
                "responses",
                path,
                DiagnosticAction::Mapped,
                DiagnosticSeverity::Warning,
                "responses.role.unknown",
                "未知 role 已映射为 user",
                Some(role.to_string()),
            ));
            InternalRole::User
        }
    }
}

fn decode_content(
    value: &Value,
    path: &str,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<Vec<InternalContentPart>, AppError> {
    if let Some(text) = value.as_str() {
        return Ok(vec![InternalContentPart::Text(text.to_string())]);
    }

    let Some(parts) = value.as_array() else {
        diagnostics.push(BridgeDiagnostic::new(
            "responses",
            path,
            DiagnosticAction::Textified,
            DiagnosticSeverity::Info,
            "responses.content.non_text",
            "非文本 content 已转为 JSON 字符串",
            value_kind(value),
        ));
        return Ok(vec![InternalContentPart::Text(value_to_text(value))]);
    };
    let text_parts = parts
        .iter()
        .filter_map(|part| decode_content_part_text(part).map(InternalContentPart::Text))
        .collect::<Vec<_>>();

    if text_parts.is_empty() {
        return Ok(parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                diagnostics.push(BridgeDiagnostic::new(
                    "responses",
                    format!("{path}/{index}"),
                    DiagnosticAction::Textified,
                    DiagnosticSeverity::Info,
                    "responses.content_part.non_text",
                    "非文本 content part 已转为 JSON 字符串",
                    part.get("type").and_then(Value::as_str).map(str::to_string),
                ));
                InternalContentPart::Text(value_to_text(part))
            })
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

fn value_kind(value: &Value) -> Option<String> {
    Some(
        match value {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::decode_responses_request;
    use crate::bridge::{
        diagnostics::{DiagnosticAction, DiagnosticPhase, DiagnosticSeverity},
        internal::{InternalContentPart, InternalRole},
    };

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
    fn decodes_responses_capability_request_flags() {
        let request = decode_responses_request(json!({
            "model": "deepseek-chat",
            "input": "hello",
            "stream": true,
            "parallel_tool_calls": true,
            "stream_options": {
                "include_usage": true
            }
        }))
        .expect("decode responses request");

        assert!(request.parallel_tool_calls_requested);
        assert!(request.streaming_usage_requested);
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
    fn decodes_responses_extensions_without_rejecting_request() {
        let request = decode_responses_request(json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "role": "planner",
                    "content": [
                        {
                            "type": "input_image",
                            "image_url": "https://example.com/image.png"
                        }
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": { "path": "Cargo.toml" }
                }
            ],
            "tools": [
                {
                    "type": "web_search_preview"
                },
                {
                    "type": "function",
                    "name": "read_file"
                }
            ]
        }))
        .expect("decode responses request");

        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name, "read_file");
        assert_eq!(request.messages[0].role, InternalRole::User);
        assert_eq!(
            request.messages[0].content,
            vec![InternalContentPart::Text(
                r#"{"image_url":"https://example.com/image.png","type":"input_image"}"#.to_string()
            )]
        );
        assert_eq!(request.messages[1].role, InternalRole::Assistant);
        assert_eq!(request.messages[1].tool_calls.len(), 1);
        assert_eq!(request.messages[1].tool_calls[0].id, "call_1");
        assert_eq!(request.messages[1].tool_calls[0].name, "read_file");
        assert_eq!(
            request.messages[1].tool_calls[0].arguments,
            r#"{"path":"Cargo.toml"}"#
        );
    }

    #[test]
    fn records_diagnostics_for_responses_compatibility_actions() {
        let decoded = super::decode_responses_request_with_diagnostics(json!({
            "model": "deepseek-chat",
            "input": [
                {
                    "role": "planner",
                    "content": [
                        {
                            "type": "input_image",
                            "image_url": "https://example.com/image.png"
                        }
                    ]
                },
                {
                    "type": "function_call",
                    "name": "read_file",
                    "arguments": { "path": "Cargo.toml" }
                }
            ],
            "tools": [
                {
                    "type": "web_search_preview"
                },
                {
                    "type": "function",
                    "name": "read_file"
                }
            ]
        }))
        .expect("decode responses request");

        assert_eq!(decoded.request.messages[0].role, InternalRole::User);
        assert_eq!(decoded.request.messages[1].tool_calls[0].id, "call_unknown");
        assert_eq!(
            decoded
                .diagnostics
                .iter()
                .map(|diagnostic| (
                    diagnostic.phase.clone(),
                    diagnostic.action.clone(),
                    diagnostic.severity.clone(),
                    diagnostic.code.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    DiagnosticPhase::Decode,
                    DiagnosticAction::Ignored,
                    DiagnosticSeverity::Warning,
                    "responses.tool.unsupported"
                ),
                (
                    DiagnosticPhase::Decode,
                    DiagnosticAction::Mapped,
                    DiagnosticSeverity::Warning,
                    "responses.role.unknown"
                ),
                (
                    DiagnosticPhase::Decode,
                    DiagnosticAction::Textified,
                    DiagnosticSeverity::Info,
                    "responses.content_part.non_text"
                ),
                (
                    DiagnosticPhase::Decode,
                    DiagnosticAction::Defaulted,
                    DiagnosticSeverity::Warning,
                    "responses.function_call.call_id_missing"
                ),
                (
                    DiagnosticPhase::Decode,
                    DiagnosticAction::Textified,
                    DiagnosticSeverity::Info,
                    "responses.function_call.arguments_non_string"
                )
            ]
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
