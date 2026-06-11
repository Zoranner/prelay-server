use serde_json::{json, Value};

use crate::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalRequest,
        InternalResponse, InternalRole, InternalUsage,
    },
    error::AppError,
};

pub fn encode_responses_request(request: &InternalRequest) -> Value {
    let mut value = json!({
        "model": request.model,
        "stream": request.stream,
        "input": request.messages.iter().map(encode_message).collect::<Vec<_>>(),
    });
    if let Some(max_tokens) = request.max_tokens {
        value["max_output_tokens"] = json!(max_tokens);
    }
    if !request.tools.is_empty() {
        value["tools"] = json!(request.tools.iter().map(encode_tool).collect::<Vec<_>>());
    }
    value
}

fn encode_message(message: &InternalMessage) -> Value {
    if matches!(message.role, InternalRole::Tool) {
        return json!({
            "type": "function_call_output",
            "call_id": message.tool_call_id,
            "output": join_text_content(&message.content),
        });
    }
    json!({
        "role": encode_role(&message.role),
        "content": join_text_content(&message.content),
    })
}

fn encode_tool(tool: &crate::bridge::internal::InternalTool) -> Value {
    let mut value = json!({
        "type": "function",
        "name": tool.name,
        "parameters": tool.input_schema,
    });
    if let Some(description) = &tool.description {
        value["description"] = json!(description);
    }
    value
}

pub fn decode_responses_response(value: Value) -> Result<InternalResponse, AppError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_unknown")
        .to_string();
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("Responses 上游响应缺少 output".to_string()))?
        .iter()
        .filter_map(decode_output_item)
        .collect::<Vec<_>>();

    Ok(InternalResponse {
        id,
        model,
        output,
        usage: decode_usage(value.get("usage")),
    })
}

fn decode_output_item(item: &Value) -> Option<InternalOutputItem> {
    match item.get("type").and_then(Value::as_str)? {
        "message" => Some(InternalOutputItem::Message {
            id: item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("msg_0")
                .to_string(),
            role: item
                .get("role")
                .and_then(Value::as_str)
                .map(decode_role)
                .unwrap_or(InternalRole::Assistant),
            content: decode_message_content(item.get("content"))?,
        }),
        "function_call" => Some(InternalOutputItem::FunctionToolCall {
            id: item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)?
                .to_string(),
            name: item.get("name").and_then(Value::as_str)?.to_string(),
            arguments: item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string(),
            reasoning_content: None,
        }),
        _ => None,
    }
}

fn decode_message_content(content: Option<&Value>) -> Option<Vec<InternalContentPart>> {
    let parts = content?.as_array()?;
    let text = parts
        .iter()
        .filter(|part| {
            part.get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "output_text")
        })
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    Some(vec![InternalContentPart::Text(text)])
}

fn decode_role(role: &str) -> InternalRole {
    match role {
        "user" => InternalRole::User,
        "system" | "developer" => InternalRole::System,
        "tool" => InternalRole::Tool,
        _ => InternalRole::Assistant,
    }
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
        input_tokens: usage.get("input_tokens").and_then(Value::as_i64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_i64),
        reasoning_tokens: usage
            .get("output_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_i64),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{decode_responses_response, encode_responses_request};
    use crate::bridge::internal::{
        InternalContentPart, InternalMessage, InternalRequest, InternalRole, InternalTool,
    };

    #[test]
    fn encodes_internal_request_to_responses_shape() {
        let encoded = encode_responses_request(&InternalRequest {
            model: "gpt-4.1".to_string(),
            stream: false,
            max_tokens: Some(128),
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

        assert_eq!(encoded["model"], "gpt-4.1");
        assert_eq!(encoded["stream"], false);
        assert_eq!(encoded["max_output_tokens"], 128);
        assert_eq!(encoded["input"][0]["role"], "user");
        assert_eq!(encoded["input"][0]["content"], "hello");
    }

    #[test]
    fn encodes_internal_tools_to_responses_tools() {
        let encoded = encode_responses_request(&InternalRequest {
            model: "gpt-4.1".to_string(),
            stream: false,
            max_tokens: None,
            previous_response_id: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
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
            messages: vec![InternalMessage {
                role: InternalRole::User,
                content: vec![InternalContentPart::Text("read".to_string())],
                tool_call_id: None,
                tool_calls: Vec::new(),
                reasoning_content: None,
            }],
        });

        assert_eq!(encoded["tools"][0]["type"], "function");
        assert_eq!(encoded["tools"][0]["name"], "read_file");
        assert_eq!(encoded["tools"][0]["description"], "Read a file");
        assert_eq!(
            encoded["tools"][0]["parameters"]["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn decodes_responses_text_response_to_internal_response() {
        let response = decode_responses_response(json!({
            "id": "resp_123",
            "model": "gpt-4.1",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "role": "assistant",
                    "content": [
                        { "type": "output_text", "text": "hello" }
                    ]
                }
            ],
            "usage": {
                "input_tokens": 3,
                "output_tokens": 4,
                "output_tokens_details": { "reasoning_tokens": 1 }
            }
        }))
        .expect("decode responses response");

        assert_eq!(response.id, "resp_123");
        assert_eq!(response.model, "gpt-4.1");
        assert_eq!(response.output[0].text_content().as_deref(), Some("hello"));
        assert_eq!(response.usage.expect("usage").reasoning_tokens, Some(1));
    }

    #[test]
    fn decodes_only_output_text_content_blocks() {
        let response = decode_responses_response(json!({
            "id": "resp_123",
            "model": "gpt-4.1",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "role": "assistant",
                    "content": [
                        { "type": "refusal", "refusal": "no" },
                        { "type": "output_text", "text": "hello" }
                    ]
                }
            ]
        }))
        .expect("decode responses response");

        assert_eq!(response.output[0].text_content().as_deref(), Some("hello"));
    }

    #[test]
    fn decodes_responses_function_call_to_internal_response() {
        let response = decode_responses_response(json!({
            "id": "resp_123",
            "model": "gpt-4.1",
            "output": [
                {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"Cargo.toml\"}"
                }
            ]
        }))
        .expect("decode responses response");

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
