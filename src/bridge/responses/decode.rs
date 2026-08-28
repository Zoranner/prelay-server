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
#[path = "decode_tests.rs"]
mod tests;
