use serde_json::json;

use super::{
    conversation_events_from_responses_request, events_from_chat_request,
    events_from_internal_request, events_from_internal_response,
};
use crate::{
    codec::internal::{
        InternalContentPart, InternalMessage, InternalOutputItem, InternalRequest,
        InternalResponse, InternalRole, InternalToolCall,
    },
    domain::activity::{ContentDraft, ContentEventBody, RolloutEventBody},
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

fn content_event(draft: &ContentDraft) -> &ContentEventBody {
    &draft.body
}

#[test]
fn responses_request_preserves_message_phase_refusal_and_media_placeholders() {
    let payload = json!({
        "instructions": "system rules",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "describe this" },
                    {
                        "type": "input_image",
                        "image_url": "data:image/png;base64,c2VjcmV0LWJ5dGVz",
                        "file_id": "file_private",
                        "detail": "high"
                    }
                ]
            },
            {
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "content": [
                    { "type": "output_text", "text": "answer" },
                    { "type": "refusal", "refusal": "cannot comply" }
                ]
            }
        ]
    });
    let input_items = payload["input"].as_array().expect("input items");

    let events = conversation_events_from_responses_request(&payload, input_items);
    let events = events.iter().map(content_event).collect::<Vec<_>>();
    assert_eq!(events.len(), 3);
    let ContentEventBody::Message(system) = events[0] else {
        panic!("expected system message");
    };
    assert_eq!(system.role, "system");
    assert_eq!(system.content, json!("system rules"));
    let ContentEventBody::Message(user) = events[1] else {
        panic!("expected user message");
    };
    assert_eq!(
        user.content,
        json!([
            { "type": "input_text", "text": "describe this" },
            { "type": "input_image", "omitted": true }
        ])
    );
    let ContentEventBody::Message(assistant) = events[2] else {
        panic!("expected assistant message");
    };
    assert_eq!(assistant.message_phase.as_deref(), Some("final_answer"));
    assert_eq!(
        assistant.content,
        json!([
            { "type": "output_text", "text": "answer" },
            { "type": "refusal", "text": "cannot comply" }
        ])
    );
    let serialized = serde_json::to_string(&events).expect("serialize content events");
    assert!(!serialized.contains("c2VjcmV0LWJ5dGVz"));
    assert!(!serialized.contains("file_private"));
    assert!(!serialized.contains("\"detail\""));
}

#[test]
fn responses_request_keeps_reasoning_summary_and_content_without_encrypted_only_items() {
    let payload = json!({
        "input": [
            {
                "type": "reasoning",
                "summary": [
                    { "type": "summary_text", "text": "short summary" }
                ],
                "content": [
                    { "type": "reasoning_text", "text": "detailed reasoning" }
                ],
                "encrypted_content": "encrypted-secret"
            },
            {
                "type": "reasoning",
                "encrypted_content": "encrypted-only-secret"
            }
        ]
    });
    let input_items = payload["input"].as_array().expect("input items");

    let events = conversation_events_from_responses_request(&payload, input_items);
    assert_eq!(events.len(), 1);
    let ContentEventBody::Reasoning(reasoning) = &events[0].body else {
        panic!("expected reasoning event");
    };
    assert_eq!(reasoning.summary.as_deref(), Some("short summary"));
    assert_eq!(reasoning.content.as_deref(), Some("detailed reasoning"));
    let serialized = serde_json::to_string(&events).expect("serialize content events");
    assert!(!serialized.contains("encrypted-secret"));
    assert!(!serialized.contains("encrypted-only-secret"));
}

#[test]
fn responses_request_keeps_all_tool_calls_and_results() {
    let payload = json!({
        "input": [
            {
                "type": "function_call",
                "call_id": "call_fn",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"ls\"}"
            },
            {
                "type": "custom_tool_call",
                "call_id": "call_custom",
                "name": "apply_patch",
                "input": "*** Begin Patch"
            },
            {
                "type": "local_shell_call",
                "call_id": "call_shell",
                "action": { "type": "exec", "command": ["ls"] }
            },
            {
                "type": "web_search_call",
                "id": "ws_1",
                "action": { "type": "search", "query": "docs" }
            },
            {
                "type": "function_call_output",
                "call_id": "call_fn",
                "output": "function result"
            },
            {
                "type": "custom_tool_call_output",
                "call_id": "call_custom",
                "output": "custom result"
            },
            {
                "type": "local_shell_call_output",
                "call_id": "call_shell",
                "output": { "stdout": "shell result" }
            }
        ]
    });
    let input_items = payload["input"].as_array().expect("input items");

    let events = conversation_events_from_responses_request(&payload, input_items);
    let tool_calls = events
        .iter()
        .filter_map(|event| match &event.body {
            ContentEventBody::ToolCall(tool_call) => Some(tool_call),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_calls.len(), 4);
    assert_eq!(tool_calls[0].tool_call_id, "call_fn");
    assert_eq!(tool_calls[0].name, "exec_command");
    assert_eq!(tool_calls[1].name, "apply_patch");
    assert_eq!(tool_calls[2].name, "local_shell_call");
    assert!(tool_calls[2].arguments.get("action").is_some());
    assert_eq!(tool_calls[3].name, "web_search_call");
    assert!(tool_calls[3].arguments.get("action").is_some());

    let tool_results = events
        .iter()
        .filter_map(|event| match &event.body {
            ContentEventBody::ToolResult(tool_result) => Some(tool_result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_results.len(), 3);
    assert_eq!(tool_results[0].tool_call_id, "call_fn");
    assert_eq!(tool_results[0].content, json!("function result"));
    assert_eq!(tool_results[1].content, json!("custom result"));
    assert_eq!(tool_results[2].content, json!({ "stdout": "shell result" }));
}
