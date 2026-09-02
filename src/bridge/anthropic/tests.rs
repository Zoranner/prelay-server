use serde_json::json;

use super::{decode_anthropic_request, validate_anthropic_bridge_payload};
use crate::bridge::internal::{InternalContentPart, InternalRole};

#[test]
fn decodes_text_messages_to_internal_request() {
    let request = decode_anthropic_request(json!({
        "model": "deepseek-chat",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "hello" },
                    { "type": "text", "text": "world" }
                ]
            }
        ]
    }))
    .expect("decode anthropic request");

    assert_eq!(request.model, "deepseek-chat");
    assert!(!request.stream);
    assert_eq!(request.max_tokens, Some(1024));
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, InternalRole::User);
    assert_eq!(
        request.messages[0].content,
        vec![
            InternalContentPart::Text("hello".to_string()),
            InternalContentPart::Text("world".to_string())
        ]
    );
}

#[test]
fn preserves_anthropic_thinking_and_tool_choice_for_bridge() {
    let request = decode_anthropic_request(json!({
        "model": "claude-sonnet",
        "max_tokens": 1024,
        "thinking": { "type": "enabled", "budget_tokens": 512 },
        "tool_choice": { "type": "auto" },
        "messages": [{ "role": "user", "content": "hello" }]
    }))
    .expect("decode anthropic request");

    assert_eq!(
        request.reasoning,
        Some(json!({ "type": "enabled", "budget_tokens": 512 }))
    );
    assert_eq!(request.tool_choice, Some(json!("auto")));
}

#[test]
fn rejects_anthropic_features_that_cannot_be_bridged() {
    let error = validate_anthropic_bridge_payload(&json!({
        "model": "claude-sonnet",
        "max_tokens": 1024,
        "temperature": 0.2,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "image",
                "source": { "type": "url", "url": "https://example.com/a.png" }
            }]
        }]
    }))
    .expect_err("non-text Anthropic features must not be silently downgraded");

    assert!(format!("{error:?}").contains("Anthropic bridge does not support"));
}

#[test]
fn decodes_system_prompt_as_system_message() {
    let request = decode_anthropic_request(json!({
        "model": "deepseek-chat",
        "max_tokens": 1024,
        "system": "Be concise.",
        "messages": [
            {
                "role": "user",
                "content": "hello"
            }
        ]
    }))
    .expect("decode anthropic request");

    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.messages[0].role, InternalRole::System);
    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text("Be concise.".to_string())]
    );
    assert_eq!(request.messages[1].role, InternalRole::User);
}

#[test]
fn decodes_anthropic_tools_to_internal_tools() {
    let request = decode_anthropic_request(json!({
        "model": "deepseek-chat",
        "max_tokens": 1024,
        "tools": [
            {
                "name": "read_file",
                "description": "Read a file",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }
            }
        ],
        "messages": [
            { "role": "user", "content": "read Cargo.toml" }
        ]
    }))
    .expect("decode anthropic request");

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
fn decodes_anthropic_extensions_without_rejecting_request() {
    let request = decode_anthropic_request(json!({
        "model": "deepseek-chat",
        "max_tokens": 1024,
        "tools": [
            {
                "type": "server_tool"
            },
            {
                "name": "read_file",
                "parameters": {
                    "type": "object"
                }
            }
        ],
        "messages": [
            {
                "role": "planner",
                "content": [
                    {
                        "type": "image",
                        "source": {
                            "type": "url",
                            "url": "https://example.com/image.png"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "content": "missing tool_use_id should stay textual"
                    }
                ]
            }
        ]
    }))
    .expect("decode anthropic request");

    assert_eq!(request.tools.len(), 1);
    assert_eq!(request.tools[0].name, "read_file");
    assert_eq!(request.messages[0].role, InternalRole::User);
    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text(
            r#"{"source":{"type":"url","url":"https://example.com/image.png"},"type":"image"}"#
                .to_string()
        )]
    );
    assert_eq!(request.messages[1].role, InternalRole::User);
    assert_eq!(
        request.messages[1].content,
        vec![InternalContentPart::Text(
            "missing tool_use_id should stay textual".to_string()
        )]
    );
}

#[test]
fn decodes_tool_result_block_to_internal_tool_message() {
    let request = decode_anthropic_request(json!({
        "model": "deepseek-chat",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "call_1",
                        "content": "file text"
                    }
                ]
            }
        ]
    }))
    .expect("decode anthropic request");

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, InternalRole::Tool);
    assert_eq!(request.messages[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text("file text".to_string())]
    );
}
