use serde_json::{json, Value};

use crate::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalRequest,
        InternalResponse, InternalRole, InternalUsage,
    },
    error::AppError,
};

pub fn encode_ollama_chat_request(request: &InternalRequest) -> Value {
    json!({
        "model": request.model,
        "stream": request.stream,
        "messages": request.messages.iter().map(encode_message).collect::<Vec<_>>(),
    })
}

pub fn decode_ollama_chat_response(value: Value) -> Result<InternalResponse, AppError> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let content = value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("Ollama 响应缺少 message.content".to_string()))?
        .to_string();

    Ok(InternalResponse {
        id: "ollama_unknown".to_string(),
        model,
        output: vec![InternalOutputItem::Message {
            id: "msg_0".to_string(),
            role: InternalRole::Assistant,
            content: vec![InternalContentPart::Text(content)],
        }],
        usage: Some(InternalUsage {
            input_tokens: value.get("prompt_eval_count").and_then(Value::as_i64),
            output_tokens: value.get("eval_count").and_then(Value::as_i64),
            reasoning_tokens: None,
        }),
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{decode_ollama_chat_response, encode_ollama_chat_request};
    use crate::bridge::internal::{
        InternalContentPart, InternalMessage, InternalRequest, InternalRole,
    };

    #[test]
    fn encodes_internal_request_to_ollama_chat_shape() {
        let encoded = encode_ollama_chat_request(&InternalRequest {
            model: "llama3.2".to_string(),
            stream: false,
            max_tokens: None,
            previous_response_id: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
            tools: Vec::new(),
            messages: vec![InternalMessage {
                role: InternalRole::User,
                content: vec![InternalContentPart::Text("hello".to_string())],
                tool_call_id: None,
                tool_calls: Vec::new(),
                reasoning_content: None,
            }],
        });

        assert_eq!(encoded["model"], "llama3.2");
        assert_eq!(encoded["stream"], false);
        assert_eq!(encoded["messages"][0]["role"], "user");
        assert_eq!(encoded["messages"][0]["content"], "hello");
    }

    #[test]
    fn decodes_ollama_chat_response_to_internal_text_response() {
        let response = decode_ollama_chat_response(json!({
            "model": "llama3.2",
            "message": {
                "role": "assistant",
                "content": "ollama hello"
            },
            "done": true,
            "prompt_eval_count": 3,
            "eval_count": 4
        }))
        .expect("decode ollama response");

        assert_eq!(response.model, "llama3.2");
        assert_eq!(
            response.output[0].text_content().as_deref(),
            Some("ollama hello")
        );
        let usage = response.usage.expect("usage");
        assert_eq!(usage.input_tokens, Some(3));
        assert_eq!(usage.output_tokens, Some(4));
        assert_eq!(usage.reasoning_tokens, None);
    }
}
