use serde_json::{json, Value};

use crate::bridge::internal::{
    InternalContentPart, InternalOutputItem, InternalResponse, InternalUsage,
};

pub fn encode_anthropic_response(response: InternalResponse) -> Value {
    json!({
        "id": response.id,
        "type": "message",
        "role": "assistant",
        "model": response.model,
        "content": response.output.into_iter().flat_map(encode_output_item).collect::<Vec<_>>(),
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": response.usage.map(encode_usage),
    })
}

fn encode_output_item(item: InternalOutputItem) -> Vec<Value> {
    match item {
        InternalOutputItem::Message { content, .. } => {
            content.into_iter().map(encode_content_part).collect()
        }
        InternalOutputItem::FunctionToolCall { .. } => Vec::new(),
    }
}

fn encode_content_part(part: InternalContentPart) -> Value {
    match part {
        InternalContentPart::Text(text) => json!({
            "type": "text",
            "text": text,
        }),
    }
}

fn encode_usage(usage: InternalUsage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::encode_anthropic_response;
    use crate::bridge::internal::{
        InternalContentPart, InternalOutputItem, InternalResponse, InternalRole, InternalUsage,
    };

    #[test]
    fn encodes_internal_text_response_to_anthropic_message_shape() {
        let encoded = encode_anthropic_response(InternalResponse {
            id: "msg_123".to_string(),
            model: "deepseek-chat".to_string(),
            output: vec![InternalOutputItem::Message {
                id: "internal_msg".to_string(),
                role: InternalRole::Assistant,
                content: vec![InternalContentPart::Text("hello".to_string())],
            }],
            usage: Some(InternalUsage {
                input_tokens: Some(10),
                output_tokens: Some(5),
                reasoning_tokens: None,
            }),
        });

        assert_eq!(encoded["id"], "msg_123");
        assert_eq!(encoded["type"], "message");
        assert_eq!(encoded["role"], "assistant");
        assert_eq!(encoded["model"], "deepseek-chat");
        assert_eq!(encoded["content"][0]["type"], "text");
        assert_eq!(encoded["content"][0]["text"], "hello");
        assert_eq!(encoded["stop_reason"], "end_turn");
        assert_eq!(encoded["usage"]["input_tokens"], 10);
        assert_eq!(encoded["usage"]["output_tokens"], 5);
    }
}
