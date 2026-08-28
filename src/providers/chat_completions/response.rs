use serde_json::Value;

use crate::{
    bridge::internal::{
        InternalContentPart, InternalOutputItem, InternalResponse, InternalRole, InternalUsage,
    },
    error::AppError,
};

pub fn decode_chat_response(value: Value) -> Result<InternalResponse, AppError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl_unknown")
        .to_string();
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| AppError::BadRequest("上游响应缺少 choices[0].message".to_string()))?;
    let reasoning_content = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty())
        .map(str::to_string);
    let output = if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        tool_calls
            .iter()
            .filter_map(|tool_call| decode_tool_call(tool_call, reasoning_content.clone()))
            .collect::<Vec<_>>()
    } else {
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        vec![InternalOutputItem::Message {
            id: "msg_0".to_string(),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text(content)],
        }]
    };

    Ok(InternalResponse {
        id,
        model,
        output,
        usage: decode_usage(value.get("usage")),
    })
}

fn decode_tool_call(
    value: &Value,
    reasoning_content: Option<String>,
) -> Option<InternalOutputItem> {
    let id = value.get("id").and_then(Value::as_str)?.to_string();
    let function = value.get("function")?;
    let name = function.get("name").and_then(Value::as_str)?.to_string();
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_string();

    Some(InternalOutputItem::FunctionToolCall {
        id,
        name,
        arguments,
        reasoning_content,
    })
}

fn decode_usage(usage: Option<&Value>) -> Option<InternalUsage> {
    let usage = usage?;
    Some(InternalUsage {
        input_tokens: usage.get("prompt_tokens").and_then(Value::as_i64),
        output_tokens: usage.get("completion_tokens").and_then(Value::as_i64),
        reasoning_tokens: usage
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_i64),
        cache_read_tokens: usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .or_else(|| usage.get("cache_read_input_tokens"))
            .and_then(Value::as_i64),
        cache_write_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64),
    })
}
