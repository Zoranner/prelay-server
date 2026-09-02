use serde_json::Value;

use crate::{
    bridge::internal::{InternalContentPart, InternalOutputItem, InternalResponse, InternalRole},
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
        .and_then(|choice| choice.get("message"));
    let Some(message) = message else {
        if value
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            return Ok(InternalResponse {
                id,
                model,
                output: Vec::new(),
                usage: crate::bridge::usage::decode_usage(value.get("usage")),
            });
        }
        return Err(AppError::UpstreamInvalidResponse {
            message: "上游响应格式无效".to_string(),
        });
    };
    let reasoning_content = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty())
        .map(str::to_string);
    let mut output = Vec::new();
    let content = message_text(message);
    if !content.is_empty() {
        output.push(InternalOutputItem::Message {
            id: "msg_0".to_string(),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text(content)],
        });
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        output.extend(
            tool_calls
                .iter()
                .filter_map(|tool_call| decode_tool_call(tool_call, reasoning_content.clone())),
        );
    } else if let Some(function_call) = message.get("function_call") {
        output.extend(decode_legacy_function_call(function_call));
    }
    if output.is_empty() {
        output.push(InternalOutputItem::Message {
            id: "msg_0".to_string(),
            role: InternalRole::Assistant,
            content: Vec::new(),
        });
    }

    Ok(InternalResponse {
        id,
        model,
        output,
        usage: crate::bridge::usage::decode_usage(value.get("usage")),
    })
}

fn message_text(message: &Value) -> String {
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = message.get("refusal").and_then(Value::as_str) {
        return text.to_string();
    }
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").or_else(|| part.get("refusal")))
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("")
}

fn decode_legacy_function_call(value: &Value) -> Option<InternalOutputItem> {
    Some(InternalOutputItem::FunctionToolCall {
        id: "call_legacy".to_string(),
        name: value.get("name").and_then(Value::as_str)?.to_string(),
        arguments: value
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}")
            .to_string(),
        reasoning_content: None,
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::decode_chat_response;

    #[test]
    fn decodes_usage_when_chat_choices_are_empty() {
        let response = decode_chat_response(json!({
            "id": "chatcmpl_usage",
            "model": "deepseek-chat",
            "choices": [],
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 0,
                "prompt_tokens_details": { "cached_tokens": 4 }
            }
        }))
        .expect("decode usage-only chat response");

        assert!(response.output.is_empty());
        assert_eq!(response.usage.expect("usage").input_tokens, Some(11));
    }
}
