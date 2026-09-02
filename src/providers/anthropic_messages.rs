use serde_json::{json, Value};

use crate::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalRequest,
        InternalResponse, InternalRole,
    },
    error::AppError,
};

pub fn encode_anthropic_messages_request(request: &InternalRequest) -> Value {
    let mut value = json!({
        "model": request.model,
        "stream": request.stream,
        "messages": request
            .messages
            .iter()
            .filter(|message| !matches!(message.role, InternalRole::System))
            .map(encode_message)
            .collect::<Vec<_>>(),
    });
    if let Some(max_tokens) = request.max_tokens {
        value["max_tokens"] = json!(max_tokens);
    }
    if let Some(system) = request
        .messages
        .iter()
        .find(|message| matches!(message.role, InternalRole::System))
    {
        value["system"] = json!(join_text_content(&system.content));
    }
    if !request.tools.is_empty() {
        value["tools"] = json!(request
            .tools
            .iter()
            .map(|tool| json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            }))
            .collect::<Vec<_>>());
    }
    value
}

fn encode_message(message: &InternalMessage) -> Value {
    if matches!(message.role, InternalRole::Tool) {
        return json!({
            "role": "user",
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": message.tool_call_id,
                    "content": join_text_content(&message.content),
                }
            ]
        });
    }
    json!({
        "role": encode_role(&message.role),
        "content": join_text_content(&message.content),
    })
}

pub fn decode_anthropic_messages_response(value: Value) -> Result<InternalResponse, AppError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_unknown")
        .to_string();
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let content_blocks = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::UpstreamInvalidResponse {
            message: "上游响应格式无效".to_string(),
        })?;
    let mut output = decode_text_content_blocks(content_blocks);
    output.extend(content_blocks.iter().filter_map(decode_tool_use_block));

    Ok(InternalResponse {
        id,
        model,
        output,
        usage: crate::bridge::usage::decode_usage(value.get("usage")),
    })
}

fn decode_text_content_blocks(blocks: &[Value]) -> Vec<InternalOutputItem> {
    let content = blocks
        .iter()
        .filter(|block| {
            block
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "text")
        })
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .map(|text| InternalContentPart::Text(text.to_string()))
        .collect::<Vec<_>>();

    if content.is_empty() {
        Vec::new()
    } else {
        vec![InternalOutputItem::Message {
            id: "msg_0".to_string(),
            role: InternalRole::Assistant,
            content,
        }]
    }
}

fn decode_tool_use_block(block: &Value) -> Option<InternalOutputItem> {
    if block
        .get("type")
        .and_then(Value::as_str)
        .is_none_or(|kind| kind != "tool_use")
    {
        return None;
    }
    Some(InternalOutputItem::FunctionToolCall {
        id: block.get("id").and_then(Value::as_str)?.to_string(),
        name: block.get("name").and_then(Value::as_str)?.to_string(),
        arguments: block
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!({}))
            .to_string(),
        reasoning_content: None,
    })
}

fn encode_role(role: &InternalRole) -> &'static str {
    match role {
        InternalRole::User | InternalRole::Tool => "user",
        InternalRole::Assistant => "assistant",
        InternalRole::System => "system",
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

    use super::{decode_anthropic_messages_response, encode_anthropic_messages_request};
    use crate::bridge::internal::{
        InternalContentPart, InternalMessage, InternalRequest, InternalRole, InternalTool,
    };

    #[test]
    fn encodes_internal_request_to_anthropic_messages_shape() {
        let encoded = encode_anthropic_messages_request(&InternalRequest {
            model: "claude-sonnet".to_string(),
            stream: false,
            max_tokens: Some(128),
            max_completion_tokens: None,
            previous_response_id: None,
            instructions: None,
            store: true,
            reasoning: None,
            tool_choice: None,
            parallel_tool_calls: None,
            text: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
            parallel_tool_calls_requested: false,
            streaming_usage_requested: false,
            tools: vec![InternalTool {
                name: "read_file".to_string(),
                description: Some("Read a file".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    }
                }),
            }],
            messages: vec![
                InternalMessage {
                    role: InternalRole::System,
                    content: vec![InternalContentPart::Text("Be concise.".to_string())],
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                    reasoning_content: None,
                },
                InternalMessage {
                    role: InternalRole::User,
                    content: vec![InternalContentPart::Text("hello".to_string())],
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                    reasoning_content: None,
                },
            ],
        });

        assert_eq!(encoded["model"], "claude-sonnet");
        assert_eq!(encoded["stream"], false);
        assert_eq!(encoded["max_tokens"], 128);
        assert_eq!(encoded["system"], "Be concise.");
        assert_eq!(encoded["messages"][0]["role"], "user");
        assert_eq!(encoded["messages"][0]["content"], "hello");
        assert_eq!(encoded["tools"][0]["name"], "read_file");
        assert_eq!(
            encoded["tools"][0]["input_schema"]["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn encodes_internal_tool_message_to_anthropic_tool_result() {
        let encoded = encode_anthropic_messages_request(&InternalRequest {
            model: "claude-sonnet".to_string(),
            stream: false,
            max_tokens: None,
            max_completion_tokens: None,
            previous_response_id: None,
            instructions: None,
            store: true,
            reasoning: None,
            tool_choice: None,
            parallel_tool_calls: None,
            text: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
            parallel_tool_calls_requested: false,
            streaming_usage_requested: false,
            tools: Vec::new(),
            messages: vec![InternalMessage {
                role: InternalRole::Tool,
                content: vec![InternalContentPart::Text("file text".to_string())],
                tool_call_id: Some("call_1".to_string()),
                tool_calls: Vec::new(),
                reasoning_content: None,
            }],
        });

        assert_eq!(encoded["messages"][0]["role"], "user");
        assert_eq!(encoded["messages"][0]["content"][0]["type"], "tool_result");
        assert_eq!(
            encoded["messages"][0]["content"][0]["tool_use_id"],
            "call_1"
        );
        assert_eq!(encoded["messages"][0]["content"][0]["content"], "file text");
    }

    #[test]
    fn decodes_anthropic_text_response_to_internal_response() {
        let response = decode_anthropic_messages_response(json!({
            "id": "msg_123",
            "model": "claude-sonnet",
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "text", "text": "world" }
            ],
            "usage": {
                "input_tokens": 3,
                "output_tokens": 4,
                "cache_read_input_tokens": 2,
                "cache_creation_input_tokens": 1
            }
        }))
        .expect("decode anthropic response");

        assert_eq!(response.id, "msg_123");
        assert_eq!(response.model, "claude-sonnet");
        assert_eq!(
            response.output[0].text_content().as_deref(),
            Some("hello\nworld")
        );
        let usage = response.usage.expect("usage");
        assert_eq!(usage.output_tokens, Some(4));
        assert_eq!(usage.cache_read_tokens, Some(2));
        assert_eq!(usage.cache_write_tokens, Some(1));
    }

    #[test]
    fn decodes_anthropic_tool_use_to_internal_response() {
        let response = decode_anthropic_messages_response(json!({
            "id": "msg_123",
            "model": "claude-sonnet",
            "content": [
                {
                    "type": "tool_use",
                    "id": "call_1",
                    "name": "read_file",
                    "input": { "path": "Cargo.toml" }
                }
            ]
        }))
        .expect("decode anthropic response");

        assert_eq!(
            response.output[0].tool_call_name().as_deref(),
            Some("read_file")
        );
        assert_eq!(
            response.output[0].tool_call_arguments().as_deref(),
            Some("{\"path\":\"Cargo.toml\"}")
        );
    }
}
