mod request;
mod response;
mod stream;

pub use request::encode_chat_request;
pub use response::decode_chat_response;

#[cfg(test)]
pub use request::decode_chat_request;
#[cfg(test)]
pub use stream::decode_chat_sse_text_deltas;

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

        assert_eq!(encoded["messages"][0]["role"], "tool");
        assert_eq!(encoded["messages"][0]["tool_call_id"], "call_1");
        assert_eq!(encoded["messages"][0]["content"], "file text");
    }

    #[test]
    fn encodes_parallel_tool_calls_for_chat_completions() {
        let encoded = encode_chat_request(&InternalRequest {
            model: "deepseek-chat".to_string(),
            stream: false,
            max_tokens: None,
            max_completion_tokens: Some(128),
            previous_response_id: None,
            instructions: None,
            store: true,
            reasoning: None,
            tool_choice: None,
            parallel_tool_calls: Some(true),
            text: None,
            reasoning_requested: false,
            tool_choice_requested: false,
            structured_output_requested: false,
            parallel_tool_calls_requested: true,
            streaming_usage_requested: false,
            tools: Vec::new(),
            messages: Vec::new(),
        });

        assert_eq!(encoded["parallel_tool_calls"], true);
        assert_eq!(encoded["max_completion_tokens"], 128);
    }

    #[test]
    fn encodes_internal_tools_to_chat_completions_functions() {
        let encoded = encode_chat_request(&InternalRequest {
            model: "deepseek-chat".to_string(),
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
    fn decodes_chat_completions_extensions_without_rejecting_request() {
        let request = decode_chat_request(json!({
            "model": "deepseek-chat",
            "messages": [
                {
                    "role": "planner",
                    "content": [
                        {
                            "type": "image_url",
                            "image_url": { "url": "https://example.com/image.png" }
                        }
                    ]
                }
            ]
        }))
        .expect("decode chat request");

        assert_eq!(request.messages[0].role, InternalRole::User);
        assert_eq!(
            request.messages[0].content,
            vec![InternalContentPart::Text(
                r#"{"image_url":{"url":"https://example.com/image.png"},"type":"image_url"}"#
                    .to_string()
            )]
        );
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
                "prompt_tokens_details": { "cached_tokens": 4 },
                "cache_creation_input_tokens": 2,
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
        assert_eq!(usage.cache_read_tokens, Some(4));
        assert_eq!(usage.cache_write_tokens, Some(2));
    }

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
