use serde_json::{json, Value};

use crate::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalRequest,
        InternalResponse, InternalRole, InternalToolCall, InternalUsage,
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
    let output = if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        tool_calls
            .iter()
            .filter_map(decode_tool_call)
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

pub fn decode_chat_sse_text_deltas(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|value| {
            value
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn decode_tool_call(value: &Value) -> Option<InternalOutputItem> {
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
    })
}

fn encode_message(message: &InternalMessage) -> Value {
    let mut value = json!({
        "role": encode_role(&message.role),
        "content": join_text_content(&message.content),
    });
    if matches!(message.role, InternalRole::Tool) {
        if let Some(tool_call_id) = &message.tool_call_id {
            value["tool_call_id"] = json!(tool_call_id);
        }
    }
    if !message.tool_calls.is_empty() {
        value["tool_calls"] = json!(message
            .tool_calls
            .iter()
            .map(encode_tool_call)
            .collect::<Vec<_>>());
    }
    value
}

fn encode_tool_call(tool_call: &InternalToolCall) -> Value {
    json!({
        "id": tool_call.id,
        "type": "function",
        "function": {
            "name": tool_call.name,
            "arguments": tool_call.arguments,
        }
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

    use super::{decode_chat_response, decode_chat_sse_text_deltas, encode_chat_request};
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
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                InternalMessage {
                    role: InternalRole::User,
                    content: vec![
                        InternalContentPart::Text("hello".to_string()),
                        InternalContentPart::Text("world".to_string()),
                    ],
                    tool_call_id: None,
                    tool_calls: Vec::new(),
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
    fn encodes_internal_tool_message_to_chat_completions_tool_result() {
        let encoded = encode_chat_request(&InternalRequest {
            model: "deepseek-chat".to_string(),
            stream: false,
            previous_response_id: None,
            messages: vec![InternalMessage {
                role: InternalRole::Tool,
                content: vec![InternalContentPart::Text("file text".to_string())],
                tool_call_id: Some("call_1".to_string()),
                tool_calls: Vec::new(),
            }],
        });

        assert_eq!(encoded["messages"][0]["role"], "tool");
        assert_eq!(encoded["messages"][0]["tool_call_id"], "call_1");
        assert_eq!(encoded["messages"][0]["content"], "file text");
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

    #[test]
    fn decodes_chat_tool_calls_to_internal_output_items() {
        let response = decode_chat_response(json!({
            "id": "chatcmpl_tools",
            "model": "deepseek-chat",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "read_file",
                                    "arguments": "{\"path\":\"Cargo.toml\"}"
                                }
                            }
                        ]
                    }
                }
            ]
        }))
        .expect("decode chat response");

        assert_eq!(response.output.len(), 1);
        assert_eq!(
            response.output[0].tool_call_name().as_deref(),
            Some("read_file")
        );
        assert_eq!(
            response.output[0].tool_call_arguments().as_deref(),
            Some("{\"path\":\"Cargo.toml\"}")
        );
    }

    #[test]
    fn decodes_chat_sse_text_deltas() {
        let deltas = decode_chat_sse_text_deltas(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
             data: [DONE]\n\n",
        );

        assert_eq!(deltas, vec!["hel".to_string(), "lo".to_string()]);
    }
}
