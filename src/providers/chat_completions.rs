use serde_json::{json, Value};

use crate::{
    bridge::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalRequest,
        InternalResponse, InternalRole, InternalTool, InternalToolCall, InternalUsage,
    },
    error::AppError,
};

pub fn decode_chat_request(value: Value) -> Result<InternalRequest, AppError> {
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
    let reasoning_requested =
        value.get("reasoning").is_some() || value.get("reasoning_effort").is_some();
    let tool_choice_requested = value.get("tool_choice").is_some();
    let structured_output_requested = value
        .pointer("/response_format/type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "text");
    let parallel_tool_calls_requested = value
        .get("parallel_tool_calls")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let streaming_usage_requested = value
        .pointer("/stream_options/include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("messages 必须是数组".to_string()))?
        .iter()
        .map(decode_message)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(InternalRequest {
        model,
        stream,
        max_tokens,
        previous_response_id: None,
        reasoning_requested,
        tool_choice_requested,
        structured_output_requested,
        parallel_tool_calls_requested,
        streaming_usage_requested,
        tools: decode_tools(value.get("tools"))?,
        messages,
    })
}

pub fn encode_chat_request(request: &InternalRequest) -> Value {
    let mut value = json!({
        "model": request.model,
        "stream": request.stream,
        "messages": request.messages.iter().map(encode_message).collect::<Vec<_>>(),
    });
    if let Some(max_tokens) = request.max_tokens {
        value["max_tokens"] = json!(max_tokens);
    }
    if !request.tools.is_empty() {
        value["tools"] = json!(request.tools.iter().map(encode_tool).collect::<Vec<_>>());
    }
    value
}

pub fn encode_chat_response(response: InternalResponse) -> Value {
    let message = response
        .output
        .first()
        .map(encode_output_item)
        .unwrap_or_else(|| json!({ "role": "assistant", "content": "" }));
    let mut value = json!({
        "id": response.id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": response.model,
        "choices": [
            {
                "index": 0,
                "message": message,
                "finish_reason": "stop",
            }
        ],
    });
    if let Some(usage) = response.usage {
        let mut usage_value = json!({
            "prompt_tokens": usage.input_tokens.unwrap_or(0),
            "completion_tokens": usage.output_tokens.unwrap_or(0),
            "total_tokens": usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0),
        });
        if let Some(reasoning_tokens) = usage.reasoning_tokens {
            usage_value["completion_tokens_details"] = json!({
                "reasoning_tokens": reasoning_tokens,
            });
        }
        value["usage"] = usage_value;
    }
    value
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

#[cfg(test)]
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

fn decode_message(value: &Value) -> Result<InternalMessage, AppError> {
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .map(decode_role)
        .transpose()?
        .ok_or_else(|| AppError::BadRequest("message.role 不能为空".to_string()))?;
    let content = value
        .get("content")
        .map(decode_content)
        .transpose()?
        .unwrap_or_default();
    let tool_calls = value
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|tool_calls| {
            tool_calls
                .iter()
                .filter_map(decode_internal_tool_call)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(InternalMessage {
        role,
        content,
        tool_call_id: value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_calls,
        reasoning_content: value
            .get("reasoning_content")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn decode_content(value: &Value) -> Result<Vec<InternalContentPart>, AppError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(text) = value.as_str() {
        return Ok(vec![InternalContentPart::Text(text.to_string())]);
    }
    let parts = value
        .as_array()
        .ok_or_else(|| AppError::BadRequest("message.content 必须是字符串或数组".to_string()))?;
    parts
        .iter()
        .map(|part| {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                return Ok(InternalContentPart::Text(text.to_string()));
            }
            if part.get("type").and_then(Value::as_str) == Some("text") {
                return part
                    .get("content")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("text").and_then(Value::as_str))
                    .map(|text| InternalContentPart::Text(text.to_string()))
                    .ok_or_else(|| AppError::BadRequest("文本内容块缺少 text".to_string()));
            }

            Err(AppError::BadRequest(
                "message.content 只支持文本内容块".to_string(),
            ))
        })
        .collect()
}

fn decode_tools(value: Option<&Value>) -> Result<Vec<InternalTool>, AppError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let tools = value
        .as_array()
        .ok_or_else(|| AppError::BadRequest("tools 必须是数组".to_string()))?;
    tools
        .iter()
        .map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| AppError::BadRequest("tool.function.name 不能为空".to_string()))?
                .to_string();
            let description = function
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string);
            let input_schema = function
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object" }));

            Ok(InternalTool {
                name,
                description,
                input_schema,
            })
        })
        .collect()
}

fn decode_internal_tool_call(value: &Value) -> Option<InternalToolCall> {
    let id = value.get("id").and_then(Value::as_str)?.to_string();
    let function = value.get("function")?;
    let name = function.get("name").and_then(Value::as_str)?.to_string();
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_string();

    Some(InternalToolCall {
        id,
        name,
        arguments,
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

fn encode_output_item(item: &InternalOutputItem) -> Value {
    match item {
        InternalOutputItem::Message { role, content, .. } => json!({
            "role": encode_role(role),
            "content": join_text_content(content),
        }),
        InternalOutputItem::FunctionToolCall {
            id,
            name,
            arguments,
            reasoning_content,
        } => {
            let mut message = json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments,
                        }
                    }
                ],
            });
            if let Some(reasoning_content) = reasoning_content {
                message["reasoning_content"] = json!(reasoning_content);
            }
            message
        }
    }
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
        if matches!(message.role, InternalRole::Assistant) {
            if let Some(reasoning_content) = &message.reasoning_content {
                value["reasoning_content"] = json!(reasoning_content);
            }
        }
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

fn encode_tool(tool: &InternalTool) -> Value {
    let mut function = json!({
        "name": tool.name,
        "parameters": tool.input_schema,
    });
    if let Some(description) = &tool.description {
        function["description"] = json!(description);
    }
    json!({
        "type": "function",
        "function": function,
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

fn decode_role(role: &str) -> Result<InternalRole, AppError> {
    match role {
        "user" => Ok(InternalRole::User),
        "assistant" => Ok(InternalRole::Assistant),
        "system" | "developer" => Ok(InternalRole::System),
        "tool" => Ok(InternalRole::Tool),
        _ => Err(AppError::BadRequest(format!("不支持的消息角色: {role}"))),
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

    use super::{
        decode_chat_request, decode_chat_response, decode_chat_sse_text_deltas, encode_chat_request,
    };
    use crate::bridge::internal::{
        InternalContentPart, InternalMessage, InternalRequest, InternalRole, InternalTool,
        InternalToolCall,
    };

    #[test]
    fn encodes_internal_messages_to_chat_completions_request() {
        let encoded = encode_chat_request(&InternalRequest {
            model: "deepseek-chat".to_string(),
            stream: false,
            max_tokens: None,
            previous_response_id: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
            parallel_tool_calls_requested: false,
            streaming_usage_requested: false,
            tools: Vec::new(),
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
                    content: vec![
                        InternalContentPart::Text("hello".to_string()),
                        InternalContentPart::Text("world".to_string()),
                    ],
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                    reasoning_content: None,
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
            max_tokens: None,
            previous_response_id: None,
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

        assert_eq!(encoded["messages"][0]["role"], "tool");
        assert_eq!(encoded["messages"][0]["tool_call_id"], "call_1");
        assert_eq!(encoded["messages"][0]["content"], "file text");
    }

    #[test]
    fn encodes_internal_tools_to_chat_completions_functions() {
        let encoded = encode_chat_request(&InternalRequest {
            model: "deepseek-chat".to_string(),
            stream: false,
            max_tokens: None,
            previous_response_id: None,
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
            messages: vec![InternalMessage {
                role: InternalRole::User,
                content: vec![InternalContentPart::Text("read".to_string())],
                tool_call_id: None,
                tool_calls: Vec::new(),
                reasoning_content: None,
            }],
        });

        assert_eq!(encoded["tools"][0]["type"], "function");
        assert_eq!(encoded["tools"][0]["function"]["name"], "read_file");
        assert_eq!(
            encoded["tools"][0]["function"]["description"],
            "Read a file"
        );
        assert_eq!(
            encoded["tools"][0]["function"]["parameters"]["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn decodes_chat_completions_text_content_parts_to_internal_request() {
        let request = decode_chat_request(json!({
            "model": "deepseek-chat",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "hello" },
                        { "text": "world" }
                    ]
                }
            ]
        }))
        .expect("decode chat request");

        assert_eq!(request.messages[0].content.len(), 2);
        assert_eq!(
            request.messages[0].content,
            vec![
                InternalContentPart::Text("hello".to_string()),
                InternalContentPart::Text("world".to_string())
            ]
        );
    }

    #[test]
    fn decodes_chat_completions_capability_request_flags() {
        let request = decode_chat_request(json!({
            "model": "deepseek-chat",
            "stream": true,
            "parallel_tool_calls": true,
            "stream_options": {
                "include_usage": true
            },
            "messages": [
                {
                    "role": "user",
                    "content": "hello"
                }
            ]
        }))
        .expect("decode chat request");

        assert!(request.parallel_tool_calls_requested);
        assert!(request.streaming_usage_requested);
    }

    #[test]
    fn rejects_chat_completions_non_text_content_parts() {
        let error = decode_chat_request(json!({
            "model": "deepseek-chat",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "image_url",
                            "image_url": { "url": "https://example.com/image.png" }
                        }
                    ]
                }
            ]
        }))
        .expect_err("non-text content should fail");

        assert!(format!("{error:?}").contains("只支持文本内容块"));
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
    fn decodes_chat_tool_call_reasoning_content_to_internal_output_items() {
        let response = decode_chat_response(json!({
            "id": "chatcmpl_tools",
            "model": "deepseek-chat",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "reasoning_content": "Need to inspect the file first.",
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

        assert_eq!(
            response.output[0].tool_call_reasoning_content().as_deref(),
            Some("Need to inspect the file first.")
        );
    }

    #[test]
    fn encodes_assistant_tool_calls_with_reasoning_content() {
        let encoded = encode_chat_request(&InternalRequest {
            model: "deepseek-chat".to_string(),
            stream: false,
            max_tokens: None,
            previous_response_id: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
            parallel_tool_calls_requested: false,
            streaming_usage_requested: false,
            tools: Vec::new(),
            messages: vec![InternalMessage {
                role: InternalRole::Assistant,
                content: Vec::new(),
                tool_call_id: None,
                tool_calls: vec![InternalToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    arguments: "{\"path\":\"Cargo.toml\"}".to_string(),
                }],
                reasoning_content: Some("Need to inspect the file first.".to_string()),
            }],
        });

        assert_eq!(encoded["messages"][0]["role"], "assistant");
        assert_eq!(
            encoded["messages"][0]["reasoning_content"],
            "Need to inspect the file first."
        );
        assert_eq!(encoded["messages"][0]["tool_calls"][0]["id"], "call_1");
    }

    #[test]
    fn does_not_encode_reasoning_content_for_plain_assistant_text() {
        let encoded = encode_chat_request(&InternalRequest {
            model: "deepseek-chat".to_string(),
            stream: false,
            max_tokens: None,
            previous_response_id: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
            parallel_tool_calls_requested: false,
            streaming_usage_requested: false,
            tools: Vec::new(),
            messages: vec![InternalMessage {
                role: InternalRole::Assistant,
                content: vec![InternalContentPart::Text("done".to_string())],
                tool_call_id: None,
                tool_calls: Vec::new(),
                reasoning_content: Some("hidden".to_string()),
            }],
        });

        assert!(encoded["messages"][0].get("reasoning_content").is_none());
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
