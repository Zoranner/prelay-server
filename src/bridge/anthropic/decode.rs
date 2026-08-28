use serde_json::Value;

use crate::{
    bridge::{
        diagnostics::{BridgeDiagnostic, DecodedRequest, DiagnosticAction, DiagnosticSeverity},
        internal::{
            InternalContentPart, InternalMessage, InternalRequest, InternalRole, InternalTool,
        },
    },
    error::AppError,
};

#[cfg(test)]
pub fn decode_anthropic_request(value: Value) -> Result<InternalRequest, AppError> {
    Ok(decode_anthropic_request_with_diagnostics(value)?.request)
}

pub fn decode_anthropic_request_with_diagnostics(value: Value) -> Result<DecodedRequest, AppError> {
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
            content: decode_content(system, "/system", &mut diagnostics)?,
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        });
    }
    decoded.extend(
        messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                decode_message(message, &format!("/messages/{index}"), &mut diagnostics)
            })
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
        tools: decode_tools(value.get("tools"), &mut diagnostics)?,
        messages: decoded,
    };
    Ok(DecodedRequest {
        request,
        diagnostics,
    })
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
    let Some(name) = tool
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
    else {
        diagnostics.push(BridgeDiagnostic::new(
            "anthropic_messages",
            format!("/tools/{index}"),
            DiagnosticAction::Ignored,
            DiagnosticSeverity::Warning,
            "anthropic.tool.unsupported",
            "跳过无法映射为 function tool 的工具定义",
            tool.get("type").and_then(Value::as_str).map(str::to_string),
        ));
        return None;
    };
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

fn decode_message(
    value: &Value,
    path: &str,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<InternalMessage, AppError> {
    if let Some(tool_message) = decode_tool_result_message(value, path, diagnostics)? {
        return Ok(tool_message);
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
        tool_call_id: None,
        tool_calls: Vec::new(),
        reasoning_content: None,
    })
}

fn decode_tool_result_message(
    value: &Value,
    path: &str,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<Option<InternalMessage>, AppError> {
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
        diagnostics.push(BridgeDiagnostic::new(
            "anthropic_messages",
            format!("{path}/content"),
            DiagnosticAction::Textified,
            DiagnosticSeverity::Warning,
            "anthropic.tool_result.missing_tool_use_id",
            "tool_result 缺少 tool_use_id，已按普通文本内容处理",
            Some("tool_result".to_string()),
        ));
        return Ok(None);
    };

    Ok(Some(InternalMessage {
        role: InternalRole::Tool,
        content: decode_tool_result_content(
            tool_result.get("content"),
            &format!("{path}/content"),
            diagnostics,
        )?,
        tool_call_id: Some(tool_call_id.to_string()),
        tool_calls: Vec::new(),
        reasoning_content: None,
    }))
}

fn decode_tool_result_content(
    value: Option<&Value>,
    path: &str,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<Vec<InternalContentPart>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if let Some(text) = value.as_str() {
        return Ok(vec![InternalContentPart::Text(text.to_string())]);
    }
    decode_content(value, path, diagnostics)
}

fn decode_role(role: &str, path: &str, diagnostics: &mut Vec<BridgeDiagnostic>) -> InternalRole {
    match role {
        "user" => InternalRole::User,
        "assistant" => InternalRole::Assistant,
        "system" | "developer" => InternalRole::System,
        "tool" => InternalRole::Tool,
        _ => {
            diagnostics.push(BridgeDiagnostic::new(
                "anthropic_messages",
                path,
                DiagnosticAction::Mapped,
                DiagnosticSeverity::Warning,
                "anthropic.role.unknown",
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
            "anthropic_messages",
            path,
            DiagnosticAction::Textified,
            DiagnosticSeverity::Info,
            "anthropic.content.non_text",
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
                    "anthropic_messages",
                    format!("{path}/{index}"),
                    DiagnosticAction::Textified,
                    DiagnosticSeverity::Info,
                    "anthropic.content_part.non_text",
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
#[path = "tests.rs"]
mod tests;
