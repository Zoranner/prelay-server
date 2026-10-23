use serde_json::json;

use super::{
    decode_responses_request, validate_internal_request_for_bridge,
    validate_responses_bridge_payload,
};
use crate::bridge::internal::{
    InternalContentPart, InternalImagePart, InternalMessage, InternalRequest, InternalRole,
    InternalToolCall,
};

#[test]
fn decodes_string_input_into_user_message() {
    let request = decode_responses_request(&json!({
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
    let request = decode_responses_request(&json!({
        "model": "deepseek-chat",
        "input": "hello",
        "max_output_tokens": 1024
    }))
    .expect("decode responses request");

    assert_eq!(request.max_tokens, Some(1024));
}

#[test]
fn decodes_responses_capability_request_flags() {
    let request = decode_responses_request(&json!({
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
    let request = decode_responses_request(&json!({
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
fn decodes_input_images_from_user_and_tool_output() {
    let request = decode_responses_request(&json!({
        "model": "deepseek-flash",
        "input": [
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "look" },
                    {
                        "type": "input_image",
                        "image_url": "data:image/png;base64,AAA",
                        "detail": "high"
                    }
                ]
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": [
                    { "type": "input_text", "text": "rendered" },
                    {
                        "type": "input_image",
                        "image_url": { "url": "data:image/png;base64,BBB" }
                    }
                ]
            }
        ]
    }))
    .expect("decode responses request");

    assert_eq!(
        request.messages[0].content,
        vec![
            InternalContentPart::Text("look".to_string()),
            InternalContentPart::Image(InternalImagePart {
                image_url: Some(json!("data:image/png;base64,AAA")),
                file_id: None,
                detail: Some("high".to_string()),
            }),
        ]
    );
    assert_eq!(request.messages[1].role, InternalRole::Tool);
    assert_eq!(
        request.messages[1].content,
        vec![
            InternalContentPart::Text("rendered".to_string()),
            InternalContentPart::Image(InternalImagePart {
                image_url: Some(json!({ "url": "data:image/png;base64,BBB" })),
                file_id: None,
                detail: None,
            }),
        ]
    );
}

#[test]
fn bridge_validation_rejects_images_in_messages_and_tool_outputs() {
    let content_error = validate_responses_bridge_payload(&json!({
        "input": [{
            "role": "user",
            "content": [
                { "type": "input_text", "text": "look" },
                { "type": "input_image", "image_url": "data:image/png;base64,AAA" }
            ]
        }]
    }))
    .expect_err("user images are not bridgeable");
    assert!(format!("{content_error:?}").contains("input_image"));

    let output_error = validate_responses_bridge_payload(&json!({
        "input": [{
            "type": "function_call_output",
            "call_id": "call_1",
            "output": [
                { "type": "input_text", "text": "rendered" },
                { "type": "input_image", "image_url": "data:image/png;base64,AAA" }
            ]
        }]
    }))
    .expect_err("tool output images are not bridgeable");
    assert!(format!("{output_error:?}").contains("input_image"));
}

#[test]
fn bridge_validation_rejects_images_loaded_from_history() {
    let request = InternalRequest {
        model: "deepseek-flash".to_string(),
        stream: false,
        max_tokens: None,
        max_completion_tokens: None,
        previous_response_id: Some("resp_previous".to_string()),
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
            role: InternalRole::User,
            content: vec![InternalContentPart::Image(InternalImagePart {
                image_url: Some(json!("data:image/png;base64,AAA")),
                file_id: None,
                detail: None,
            })],
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        }],
    };

    let error = validate_internal_request_for_bridge(&request)
        .expect_err("history images are not bridgeable");
    assert!(format!("{error:?}").contains("input images"));
}

#[test]
fn session_json_round_trip_preserves_images_and_tool_calls() {
    let messages = vec![
        InternalMessage {
            role: InternalRole::User,
            content: vec![
                InternalContentPart::Text("look".to_string()),
                InternalContentPart::Image(InternalImagePart {
                    image_url: Some(json!("data:image/png;base64,AAA")),
                    file_id: None,
                    detail: Some("high".to_string()),
                }),
            ],
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        },
        InternalMessage {
            role: InternalRole::Assistant,
            content: Vec::new(),
            tool_call_id: None,
            tool_calls: vec![InternalToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: "{\"path\":\"a.txt\"}".to_string(),
            }],
            reasoning_content: None,
        },
    ];

    let encoded = serde_json::to_string(&messages).expect("serialize session messages");
    let decoded: Vec<InternalMessage> =
        serde_json::from_str(&encoded).expect("deserialize session messages");

    assert_eq!(decoded, messages);
}

#[test]
fn decodes_function_tools_to_internal_tools() {
    let request = decode_responses_request(&json!({
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
    let request = decode_responses_request(&json!({
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
        vec![InternalContentPart::Image(InternalImagePart {
            image_url: Some(json!("https://example.com/image.png")),
            file_id: None,
            detail: None,
        })]
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
    let request = decode_responses_request(&json!({
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

#[test]
fn decodes_responses_request_options_without_input() {
    let request = decode_responses_request(&json!({
        "model": "gpt-4.1",
        "store": false,
        "instructions": "Be concise.",
        "reasoning": { "effort": "high" },
        "tool_choice": "required",
        "parallel_tool_calls": true,
        "text": { "format": { "type": "json_object" } }
    }))
    .expect("decode responses request");

    assert_eq!(request.instructions.as_deref(), Some("Be concise."));
    assert!(!request.store);
    assert_eq!(request.reasoning, Some(json!({ "effort": "high" })));
    assert_eq!(request.tool_choice, Some(json!("required")));
    assert_eq!(request.parallel_tool_calls, Some(true));
    assert_eq!(
        request.text,
        Some(json!({ "format": { "type": "json_object" } }))
    );
    assert!(request.messages.is_empty());
}

#[test]
fn rejects_responses_features_that_cannot_be_bridged() {
    let error = validate_responses_bridge_payload(&json!({
        "input": [{
            "role": "user",
            "content": [{ "type": "input_image", "image_url": "https://example.com/a.png" }]
        }],
        "tools": [{ "type": "web_search_preview" }]
    }))
    .expect_err("non-text Responses features must not be silently downgraded");

    assert!(format!("{error:?}").contains("Responses bridge does not support"));
}
