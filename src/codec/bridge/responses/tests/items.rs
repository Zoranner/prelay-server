use serde_json::json;

use super::*;

#[test]
fn decodes_messages_and_tool_items_for_chat_bridge() {
    let messages = decode_items_for_chat_bridge(&[
        json!({ "role": "user", "content": "hello" }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "checking", "annotations": [] }]
        }),
        json!({
            "type": "function_call",
            "call_id": "call_1",
            "name": "read_file",
            "arguments": "{\"path\":\"a.txt\"}"
        }),
        json!({
            "type": "function_call_output",
            "call_id": "call_1",
            "output": "file text"
        }),
    ])
    .expect("bridge items");

    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].role, InternalRole::User);
    assert_eq!(messages[1].role, InternalRole::Assistant);
    assert_eq!(
        messages[1].content,
        vec![InternalContentPart::Text("checking".to_string())]
    );
    assert_eq!(messages[2].role, InternalRole::Assistant);
    assert_eq!(
        messages[2].tool_calls,
        vec![InternalToolCall {
            id: "call_1".to_string(),
            name: "read_file".to_string(),
            arguments: "{\"path\":\"a.txt\"}".to_string(),
        }]
    );
    assert_eq!(messages[3].role, InternalRole::Tool);
    assert_eq!(messages[3].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(
        messages[3].content,
        vec![InternalContentPart::Text("file text".to_string())]
    );
}

#[test]
fn rejects_items_and_roles_that_cannot_be_bridged() {
    let error = decode_items_for_chat_bridge(&[json!({ "type": "reasoning", "id": "rs_1" })])
        .expect_err("reasoning must not be downgraded");
    assert!(matches!(
        error,
        crate::error::AppError::OpenAi {
            code: "unsupported_parameter",
            param: Some("input"),
            ..
        }
    ));

    let error = decode_items_for_chat_bridge(&[json!({
        "type": "message",
        "role": "critic",
        "content": "hello"
    })])
    .expect_err("unknown roles must not default to user");
    assert!(matches!(
        error,
        crate::error::AppError::OpenAi {
            code: "invalid_request_error",
            param: Some("input"),
            ..
        }
    ));
}
