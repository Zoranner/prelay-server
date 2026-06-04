use serde_json::Value;

use crate::{
    bridge::internal::{InternalContentPart, InternalMessage, InternalRequest, InternalRole},
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
        });
    }
    decoded.extend(
        messages
            .iter()
            .map(decode_message)
            .collect::<Result<Vec<_>, _>>()?,
    );

    Ok(InternalRequest {
        model,
        stream,
        max_tokens,
        previous_response_id: None,
        messages: decoded,
    })
}

fn decode_message(value: &Value) -> Result<InternalMessage, AppError> {
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
        tool_call_id: None,
        tool_calls: Vec::new(),
    })
}

fn decode_role(role: &str) -> Result<InternalRole, AppError> {
    match role {
        "user" => Ok(InternalRole::User),
        "assistant" => Ok(InternalRole::Assistant),
        _ => Err(AppError::BadRequest(format!("不支持的消息角色: {role}"))),
    }
}

fn decode_content(value: &Value) -> Result<Vec<InternalContentPart>, AppError> {
    if let Some(text) = value.as_str() {
        return Ok(vec![InternalContentPart::Text(text.to_string())]);
    }

    let parts = value
        .as_array()
        .ok_or_else(|| AppError::BadRequest("content 必须是字符串或数组".to_string()))?;
    let text_parts = parts
        .iter()
        .filter_map(|part| {
            part.get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "text")
                .then(|| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .map(|text| InternalContentPart::Text(text.to_string()))
                })
                .flatten()
        })
        .collect::<Vec<_>>();

    if text_parts.is_empty() {
        return Err(AppError::BadRequest("content 没有可用文本".to_string()));
    }

    Ok(text_parts)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::decode_anthropic_request;
    use crate::bridge::internal::{InternalContentPart, InternalRole};

    #[test]
    fn decodes_text_messages_to_internal_request() {
        let request = decode_anthropic_request(json!({
            "model": "deepseek-chat",
            "max_tokens": 1024,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "hello" },
                        { "type": "text", "text": "world" }
                    ]
                }
            ]
        }))
        .expect("decode anthropic request");

        assert_eq!(request.model, "deepseek-chat");
        assert!(!request.stream);
        assert_eq!(request.max_tokens, Some(1024));
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, InternalRole::User);
        assert_eq!(
            request.messages[0].content,
            vec![
                InternalContentPart::Text("hello".to_string()),
                InternalContentPart::Text("world".to_string())
            ]
        );
    }

    #[test]
    fn decodes_system_prompt_as_system_message() {
        let request = decode_anthropic_request(json!({
            "model": "deepseek-chat",
            "max_tokens": 1024,
            "system": "Be concise.",
            "messages": [
                {
                    "role": "user",
                    "content": "hello"
                }
            ]
        }))
        .expect("decode anthropic request");

        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, InternalRole::System);
        assert_eq!(
            request.messages[0].content,
            vec![InternalContentPart::Text("Be concise.".to_string())]
        );
        assert_eq!(request.messages[1].role, InternalRole::User);
    }
}
