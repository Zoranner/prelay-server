use serde_json::json;

use super::decode_responses_request;
use crate::{
    codec::{
        activity::conversation_events_from_responses_request,
        internal::{InternalContentPart, InternalRole},
    },
    domain::activity::ContentEventBody,
};

#[test]
fn decodes_reasoning_items_as_reasoning_not_user_text() {
    let payload = json!({
        "model": "deepseek-flash",
        "input": [
            {
                "type": "reasoning",
                "summary": [
                    { "type": "summary_text", "text": "inspect the protocol" },
                    { "type": "summary_text", "text": "then patch it" }
                ]
            },
            {
                "type": "message",
                "role": "user",
                "content": "hello"
            }
        ]
    });
    let request = decode_responses_request(&payload).expect("decode reasoning input");

    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.messages[0].role, InternalRole::Assistant);
    assert!(request.messages[0].content.is_empty());
    assert_eq!(
        request.messages[0].reasoning_content.as_deref(),
        Some("inspect the protocol\nthen patch it")
    );
    assert_eq!(request.messages[1].role, InternalRole::User);
    assert_eq!(
        request.messages[1].content,
        vec![InternalContentPart::Text("hello".to_string())]
    );

    let input_items = payload["input"].as_array().expect("input items");
    let events = conversation_events_from_responses_request(&payload, input_items);
    assert!(events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::Reasoning(reasoning)
            if reasoning.summary.as_deref() == Some("inspect the protocol\nthen patch it")
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::Message(message)
            if message.role == "user"
                && message.content.as_str()
                    == Some("inspect the protocol\nthen patch it")
    )));
}

#[test]
fn skips_opaque_input_items_without_coercing_them_to_user_text() {
    let request = decode_responses_request(&json!({
        "model": "deepseek-flash",
        "input": [
            { "type": "configuration_update", "reasoning": { "effort": "high" } },
            { "type": "item_reference", "id": "msg_previous" },
            { "type": "message", "role": "user", "content": "hello" }
        ]
    }))
    .expect("decode opaque input items");

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, InternalRole::User);
    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text("hello".to_string())]
    );
}

#[test]
fn ignores_encrypted_only_reasoning_items() {
    let request = decode_responses_request(&json!({
        "model": "deepseek-flash",
        "input": [
            {
                "type": "reasoning",
                "summary": [],
                "encrypted_content": "opaque"
            }
        ]
    }))
    .expect("decode encrypted reasoning");

    assert!(request.messages.is_empty());
}

#[test]
fn decodes_reasoning_content_when_summary_is_empty() {
    let request = decode_responses_request(&json!({
        "model": "deepseek-flash",
        "input": [
            {
                "type": "reasoning",
                "summary": [],
                "content": [
                    { "type": "reasoning_text", "text": "raw reasoning" }
                ]
            }
        ]
    }))
    .expect("decode reasoning content");

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, InternalRole::Assistant);
    assert_eq!(
        request.messages[0].reasoning_content.as_deref(),
        Some("raw reasoning")
    );
}

#[test]
fn omits_media_content_parts_without_turning_them_into_text() {
    let request = decode_responses_request(&json!({
        "model": "deepseek-flash",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "keep this" },
                {
                    "type": "input_file",
                    "filename": "report.pdf",
                    "file_data": "raw file bytes"
                },
                {
                    "type": "input_audio",
                    "input_audio": { "data": "raw audio bytes", "format": "wav" }
                },
                { "type": "unknown_extension", "text": "do not stringify" }
            ]
        }]
    }))
    .expect("decode message with unsupported content parts");

    assert_eq!(
        request.messages[0].content,
        vec![InternalContentPart::Text("keep this".to_string())]
    );
}
