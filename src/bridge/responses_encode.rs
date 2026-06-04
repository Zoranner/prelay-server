use serde_json::{json, Value};

use crate::bridge::internal::{
    InternalContentPart, InternalOutputItem, InternalResponse, InternalRole, InternalUsage,
};

pub fn encode_responses_response(response: InternalResponse) -> Value {
    json!({
        "id": response.id,
        "object": "response",
        "created_at": chrono::Utc::now().timestamp(),
        "status": "completed",
        "model": response.model,
        "output": response.output.into_iter().map(encode_output_item).collect::<Vec<_>>(),
        "usage": response.usage.map(encode_usage),
    })
}

fn encode_output_item(item: InternalOutputItem) -> Value {
    match item {
        InternalOutputItem::Message { id, role, content } => json!({
            "type": "message",
            "id": id,
            "status": "completed",
            "role": encode_role(role),
            "content": content.into_iter().map(encode_content_part).collect::<Vec<_>>(),
        }),
        InternalOutputItem::FunctionToolCall {
            id,
            name,
            arguments,
            ..
        } => json!({
            "type": "function_call",
            "id": id,
            "call_id": id,
            "name": name,
            "arguments": arguments,
            "status": "completed",
        }),
    }
}

fn encode_role(role: InternalRole) -> &'static str {
    match role {
        InternalRole::User => "user",
        InternalRole::Assistant => "assistant",
        InternalRole::System => "system",
        InternalRole::Tool => "tool",
    }
}

fn encode_content_part(part: InternalContentPart) -> Value {
    match part {
        InternalContentPart::Text(text) => json!({
            "type": "output_text",
            "text": text,
            "annotations": [],
        }),
    }
}

fn encode_usage(usage: InternalUsage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "total_tokens": usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0),
        "output_tokens_details": {
            "reasoning_tokens": usage.reasoning_tokens,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::encode_responses_response;
    use crate::bridge::internal::{
        InternalContentPart, InternalOutputItem, InternalResponse, InternalRole, InternalUsage,
    };

    #[test]
    fn encodes_internal_text_response_to_openai_responses_shape() {
        let encoded = encode_responses_response(InternalResponse {
            id: "resp_123".to_string(),
            model: "deepseek-chat".to_string(),
            output: vec![InternalOutputItem::Message {
                id: "msg_123".to_string(),
                role: InternalRole::Assistant,
                content: vec![InternalContentPart::Text("hello".to_string())],
            }],
            usage: Some(InternalUsage {
                input_tokens: Some(10),
                output_tokens: Some(5),
                reasoning_tokens: Some(2),
            }),
        });

        assert_eq!(encoded["id"], "resp_123");
        assert_eq!(encoded["object"], "response");
        assert_eq!(encoded["model"], "deepseek-chat");
        assert_eq!(encoded["output"][0]["type"], "message");
        assert_eq!(encoded["output"][0]["id"], "msg_123");
        assert_eq!(encoded["output"][0]["role"], "assistant");
        assert_eq!(encoded["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(encoded["output"][0]["content"][0]["text"], "hello");
        assert_eq!(encoded["usage"]["input_tokens"], 10);
        assert_eq!(encoded["usage"]["output_tokens"], 5);
        assert_eq!(
            encoded["usage"]["output_tokens_details"]["reasoning_tokens"],
            2
        );
    }

    #[test]
    fn encodes_internal_tool_call_to_openai_responses_shape() {
        let encoded = encode_responses_response(InternalResponse {
            id: "resp_123".to_string(),
            model: "deepseek-chat".to_string(),
            output: vec![InternalOutputItem::FunctionToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: "{\"path\":\"Cargo.toml\"}".to_string(),
                reasoning_content: None,
            }],
            usage: None,
        });

        assert_eq!(encoded["output"][0]["type"], "function_call");
        assert_eq!(encoded["output"][0]["call_id"], "call_1");
        assert_eq!(encoded["output"][0]["name"], "read_file");
        assert_eq!(
            encoded["output"][0]["arguments"],
            "{\"path\":\"Cargo.toml\"}"
        );
    }
}
