use serde_json::{json, Value};

use crate::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalRequest,
        InternalResponse, InternalRole, InternalUsage,
    },
    error::AppError,
};

pub fn encode_chat_request(request: &InternalRequest) -> Value {
    json!({
        "model": request.model,
        "stream": request.stream,
        "messages": request.messages.iter().map(encode_message).collect::<Vec<_>>(),
    })
}

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
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    Ok(InternalResponse {
        id,
        model,
        output: vec![InternalOutputItem::Message {
            id: "msg_0".to_string(),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text(content)],
        }],
        usage: decode_usage(value.get("usage")),
    })
}

fn encode_message(message: &InternalMessage) -> Value {
    json!({
        "role": encode_role(&message.role),
        "content": join_text_content(&message.content),
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

fn join_text_content(content: &[InternalContentPart]) -> String {
    content
        .iter()
        .map(|part| match part {
            InternalContentPart::Text(text) => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{decode_chat_response, encode_chat_request};
    use crate::bridge::internal::{
        InternalContentPart, InternalMessage, InternalRequest, InternalRole,
    };

    #[test]
    fn encodes_internal_messages_to_chat_completions_request() {
        let encoded = encode_chat_request(&InternalRequest {
            model: "deepseek-chat".to_string(),
            stream: false,
            previous_response_id: None,
            messages: vec![
                InternalMessage {
                    role: InternalRole::System,
                    content: vec![InternalContentPart::Text("Be concise.".to_string())],
                },
                InternalMessage {
                    role: InternalRole::User,
                    content: vec![
                        InternalContentPart::Text("hello".to_string()),
                        InternalContentPart::Text("world".to_string()),
                    ],
                },
            ],
        });

        assert_eq!(encoded["model"], "deepseek-chat");
        assert_eq!(encoded["stream"], false);
        assert_eq!(encoded["messages"][0]["role"], "system");
        assert_eq!(encoded["messages"][0]["content"], "Be concise.");
        assert_eq!(encoded["messages"][1]["role"], "user");
        assert_eq!(encoded["messages"][1]["content"], "hello\nworld");
    }

    #[test]
    fn decodes_chat_completions_response_to_internal_response() {
        let response = decode_chat_response(json!({
            "id": "chatcmpl_123",
            "model": "deepseek-chat",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "hello"
                    }
                }
            ],
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 7,
                "completion_tokens_details": {
                    "reasoning_tokens": 3
                }
            }
        }))
        .expect("decode chat response");

        assert_eq!(response.id, "chatcmpl_123");
        assert_eq!(response.model, "deepseek-chat");
        assert_eq!(response.output[0].text_content().as_deref(), Some("hello"));
        let usage = response.usage.expect("usage");
        assert_eq!(usage.input_tokens, Some(11));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.reasoning_tokens, Some(3));
    }
}
