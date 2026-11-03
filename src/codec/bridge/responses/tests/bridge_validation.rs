use serde_json::json;

use super::super::{decode_items_for_chat_bridge, validate_responses_bridge_payload};
use crate::codec::internal::InternalContentPart;

#[test]
fn accepts_replayed_output_messages() {
    let payload = json!({
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "content": [
                    { "type": "output_text", "text": "checking" },
                    { "type": "refusal", "refusal": "I cannot do that" }
                ]
            },
            { "role": "user", "content": "continue" }
        ]
    });

    validate_responses_bridge_payload(&payload).expect("output messages must be bridgeable");
    let items = payload["input"].as_array().expect("input items");
    let messages = decode_items_for_chat_bridge(items).expect("bridge output messages");

    assert_eq!(
        messages[0].content,
        vec![
            InternalContentPart::Text("checking".to_string()),
            InternalContentPart::Text("I cannot do that".to_string())
        ]
    );
}

#[test]
fn rejects_instruction_items() {
    let error = validate_responses_bridge_payload(&json!({
        "instructions": [
            { "role": "developer", "content": "follow the repository rules" }
        ],
        "input": "hello"
    }))
    .expect_err("instruction items cannot be represented by the chat bridge");

    assert!(format!("{error:?}").contains("instruction items"));
}

#[test]
fn rejects_assistant_message_phase() {
    let error = validate_responses_bridge_payload(&json!({
        "input": [{
            "type": "message",
            "role": "assistant",
            "phase": "commentary",
            "content": [{ "type": "output_text", "text": "checking" }]
        }]
    }))
    .expect_err("assistant message phase cannot be represented by the chat bridge");

    assert!(format!("{error:?}").contains("assistant message phase"));
}
