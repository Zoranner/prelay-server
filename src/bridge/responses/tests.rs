use serde_json::json;

use super::decode_responses_request;
use crate::bridge::internal::{InternalContentPart, InternalRole};

#[test]
fn decodes_string_input_into_user_message() {
    let request = decode_responses_request(json!({
        "model": "deepseek-chat",
        "input": "hello",
        "stream": true,
        "previous_response_id": "resp_previous"
    }))
    .expect("decode responses request");

    assert_eq!(request.model, "deepseek-chat");
    assert!(request.stream);
    assert_eq!(
        request.previous_response_id.as_deref(),
        Some("resp_previous")
    );
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, InternalRole::User);
    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text("hello".to_string())]
    );
}

#[test]
fn decodes_max_output_tokens_to_internal_max_tokens() {
    let request = decode_responses_request(json!({
        "model": "deepseek-chat",
        "input": "hello",
        "max_output_tokens": 1024
    }))
    .expect("decode responses request");

    assert_eq!(request.max_tokens, Some(1024));
}

#[test]
fn decodes_responses_capability_request_flags() {
    let request = decode_responses_request(json!({
        "model": "deepseek-chat",
        "input": "hello",
        "stream": true,
        "parallel_tool_calls": true,
        "stream_options": {
            "include_usage": true
        }
    }))
    .expect("decode responses request");

    assert!(request.parallel_tool_calls_requested);
    assert!(request.streaming_usage_requested);
}

#[test]
fn decodes_message_array_input_into_internal_messages() {
    let request = decode_responses_request(json!({
        "model": "deepseek-chat",
        "input": [
            {
                "role": "system",
                "content": "You are concise."
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" },
                    { "type": "text", "text": "world" }
                ]
            }
        ]
    }))
    .expect("decode responses request");

    assert!(!request.stream);
    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.messages[0].role, InternalRole::System);
    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text("You are concise.".to_string())]
    );
    assert_eq!(request.messages[1].role, InternalRole::User);
    assert_eq!(
        request.messages[1].content,
        vec![
            InternalContentPart::Text("hello".to_string()),
            InternalContentPart::Text("world".to_string())
        ]
    );
}

#[test]
fn decodes_function_tools_to_internal_tools() {
    let request = decode_responses_request(json!({
        "model": "deepseek-chat",
        "input": "hello",
        "tools": [
            {
                "type": "function",
                "name": "read_file",
                "description": "Read a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }
            }
        ]
    }))
    .expect("decode responses request");

    assert_eq!(request.tools.len(), 1);
    assert_eq!(request.tools[0].name, "read_file");
    assert_eq!(request.tools[0].description.as_deref(), Some("Read a file"));
    assert_eq!(request.tools[0].input_schema["type"], "object");
    assert_eq!(
        request.tools[0].input_schema["properties"]["path"]["type"],
        "string"
    );
}

#[test]
fn decodes_responses_extensions_without_rejecting_request() {
    let request = decode_responses_request(json!({
        "model": "deepseek-chat",
        "input": [
            {
                "role": "planner",
                "content": [
                    {
                        "type": "input_image",
                        "image_url": "https://example.com/image.png"
                    }
                ]
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": { "path": "Cargo.toml" }
            }
        ],
        "tools": [
            {
                "type": "web_search_preview"
            },
            {
                "type": "function",
                "name": "read_file"
            }
        ]
    }))
    .expect("decode responses request");

    assert_eq!(request.tools.len(), 1);
    assert_eq!(request.tools[0].name, "read_file");
    assert_eq!(request.messages[0].role, InternalRole::User);
    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text(
            r#"{"image_url":"https://example.com/image.png","type":"input_image"}"#.to_string()
        )]
    );
    assert_eq!(request.messages[1].role, InternalRole::Assistant);
    assert_eq!(request.messages[1].tool_calls.len(), 1);
    assert_eq!(request.messages[1].tool_calls[0].id, "call_1");
    assert_eq!(request.messages[1].tool_calls[0].name, "read_file");
    assert_eq!(
        request.messages[1].tool_calls[0].arguments,
        r#"{"path":"Cargo.toml"}"#
    );
}

#[test]
fn decodes_function_call_output_into_tool_message() {
    let request = decode_responses_request(json!({
        "model": "deepseek-chat",
        "input": [
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "{\"content\":\"file text\"}"
            }
        ]
    }))
    .expect("decode responses request");

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, InternalRole::Tool);
    assert_eq!(request.messages[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text(
            "{\"content\":\"file text\"}".to_string()
        )]
    );
}
