use serde_json::json;

use super::{
    events::{
        events_from_chat_request, events_from_internal_request, events_from_internal_response,
    },
    RolloutEventBody,
};
use crate::bridge::internal::{
    InternalContentPart, InternalMessage, InternalOutputItem, InternalRequest, InternalResponse,
    InternalRole, InternalToolCall,
};

#[test]
fn internal_request_keeps_instructions_tool_calls_and_tool_results() {
    let request = InternalRequest {
        model: "model".to_string(),
        stream: false,
        max_tokens: None,
        max_completion_tokens: None,
        previous_response_id: None,
        instructions: Some("system rules".to_string()),
        store: false,
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
                role: InternalRole::Assistant,
                content: Vec::new(),
                tool_call_id: None,
                tool_calls: vec![InternalToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    arguments: json!({"path":"Cargo.toml"}).to_string(),
                }],
                reasoning_content: None,
            },
            InternalMessage {
                role: InternalRole::Tool,
                content: vec![InternalContentPart::Text("package content".to_string())],
                tool_call_id: Some("call_1".to_string()),
                tool_calls: Vec::new(),
                reasoning_content: None,
            },
        ],
    };

    let events = events_from_internal_request(&request);
    assert!(events.iter().any(|event| matches!(
        event,
        RolloutEventBody::Message(message)
            if message.role == "system" && message.content == json!("system rules")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        RolloutEventBody::ToolCall(tool_call)
            if tool_call.tool_call_id == "call_1" && tool_call.name == "read_file"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        RolloutEventBody::ToolResult(tool_result)
            if tool_result.tool_call_id == "call_1"
                && tool_result.content == json!("package content")
    )));
}

#[test]
fn internal_response_keeps_function_calls() {
    let response = InternalResponse {
        id: "resp_1".to_string(),
        model: "model".to_string(),
        output: vec![InternalOutputItem::FunctionToolCall {
            id: "call_1".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"Cargo.toml"}).to_string(),
            reasoning_content: Some("inspect first".to_string()),
        }],
        usage: None,
    };

    let events = events_from_internal_response(&response);
    assert!(events.iter().any(|event| matches!(
        event,
        RolloutEventBody::Reasoning(reasoning) if reasoning.content == "inspect first"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        RolloutEventBody::ToolCall(tool_call)
            if tool_call.tool_call_id == "call_1"
                && tool_call.arguments.contains("Cargo.toml")
    )));
}

#[test]
fn chat_request_keeps_tool_calls_and_results() {
    let payload = json!({
        "messages": [
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": "{\"path\":\"Cargo.toml\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "package content"
            }
        ]
    });

    let events = events_from_chat_request(&payload);
    assert!(events.iter().any(|event| matches!(
        event,
        RolloutEventBody::ToolCall(tool_call)
            if tool_call.tool_call_id == "call_1" && tool_call.name == "read_file"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        RolloutEventBody::ToolResult(tool_result)
            if tool_result.tool_call_id == "call_1"
                && tool_result.content == json!("package content")
    )));
}

#[test]
fn request_events_do_not_store_inline_media_data() {
    let chat = json!({
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": "describe this" },
                {
                    "type": "image_url",
                    "image_url": { "url": "data:image/png;base64,c2VjcmV0LWJ5dGVz" }
                }
            ]
        }]
    });
    let events = events_from_chat_request(&chat);
    let serialized = serde_json::to_string(&events).expect("serialize events");
    assert!(serialized.contains("describe this"));
    assert!(!serialized.contains("c2VjcmV0LWJ5dGVz"));
}
