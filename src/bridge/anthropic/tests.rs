use serde_json::json;

use super::decode_anthropic_request;
use crate::bridge::{
    diagnostics::{DiagnosticAction, DiagnosticPhase, DiagnosticSeverity},
    internal::{InternalContentPart, InternalRole},
};

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
fn records_diagnostics_for_anthropic_compatibility_actions() {
    let decoded = super::decode_anthropic_request_with_diagnostics(json!({
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

    assert_eq!(decoded.request.tools.len(), 1);
    assert_eq!(decoded.request.messages[0].role, InternalRole::User);
    assert_eq!(
        decoded
            .diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.phase.clone(),
                diagnostic.action.clone(),
                diagnostic.severity.clone(),
                diagnostic.code.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                DiagnosticPhase::Decode,
                DiagnosticAction::Mapped,
                DiagnosticSeverity::Warning,
                "anthropic.role.unknown"
            ),
            (
                DiagnosticPhase::Decode,
                DiagnosticAction::Textified,
                DiagnosticSeverity::Info,
                "anthropic.content_part.non_text"
            ),
            (
                DiagnosticPhase::Decode,
                DiagnosticAction::Textified,
                DiagnosticSeverity::Warning,
                "anthropic.tool_result.missing_tool_use_id"
            ),
            (
                DiagnosticPhase::Decode,
                DiagnosticAction::Ignored,
                DiagnosticSeverity::Warning,
                "anthropic.tool.unsupported"
            )
        ]
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
