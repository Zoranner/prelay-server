use serde_json::json;

use super::{
    conversation_events_from_chat_request, conversation_events_from_chat_response,
    conversation_events_from_image_request, conversation_events_from_image_response,
    conversation_events_from_internal_response, conversation_events_from_responses_output,
    conversation_events_from_responses_request, conversation_events_from_stream_capture,
    conversation_events_from_stream_snapshot,
};
use crate::{
    codec::bridge::stream::StreamStatsSnapshot,
    codec::internal::{
        InternalContentPart, InternalOutputItem, InternalResponse, InternalRole, InternalUsage,
    },
    domain::activity::{
        ContentDraft, ContentEventBody, RawStreamContentCapture, RawStreamProtocol,
    },
};

fn body(draft: &ContentDraft) -> &ContentEventBody {
    &draft.body
}

#[test]
fn responses_output_preserves_reasoning_message_refusal_tools_and_usage() {
    let output = json!([
        {
            "type": "reasoning",
            "summary": [{ "type": "summary_text", "text": "summary" }],
            "content": [{ "type": "reasoning_text", "text": "reasoning" }],
            "encrypted_content": "encrypted-secret"
        },
        {
            "type": "message",
            "role": "assistant",
            "phase": "final_answer",
            "content": [
                { "type": "output_text", "text": "answer" },
                { "type": "refusal", "refusal": "no" }
            ]
        },
        {
            "type": "function_call",
            "call_id": "call_1",
            "name": "exec_command",
            "arguments": "{\"cmd\":\"ls\"}"
        },
        {
            "type": "custom_tool_call",
            "call_id": "call_2",
            "name": "apply_patch",
            "input": "*** Begin Patch"
        }
    ]);
    let usage = json!({
        "input_tokens": 3,
        "output_tokens": 4,
        "reasoning_tokens": 2,
        "input_tokens_details": { "cached_tokens": 1, "cache_write_tokens": 2 }
    });

    let events = conversation_events_from_responses_output(&output, Some(&usage));
    let events = events.iter().map(body).collect::<Vec<_>>();
    assert!(matches!(
        events[0],
        ContentEventBody::Reasoning(reasoning)
            if reasoning.summary.as_deref() == Some("summary")
                && reasoning.content.as_deref() == Some("reasoning")
    ));
    assert!(matches!(
        events[1],
        ContentEventBody::Message(message)
            if message.message_phase.as_deref() == Some("final_answer")
                && message.content[1] == json!({ "type": "refusal", "text": "no" })
    ));
    assert!(events.iter().any(|event| matches!(
        event,
        ContentEventBody::ToolCall(tool_call)
            if tool_call.tool_call_id == "call_2" && tool_call.name == "apply_patch"
    )));
    assert!(matches!(
        events.last(),
        Some(ContentEventBody::Usage(usage))
            if usage.input_tokens == Some(3)
                && usage.output_tokens == Some(4)
                && usage.reasoning_tokens == Some(2)
                && usage.cache_read_tokens == Some(1)
                && usage.cache_write_tokens == Some(2)
    ));
    let serialized = serde_json::to_string(&events).expect("serialize output events");
    assert!(!serialized.contains("encrypted-secret"));
}

#[test]
fn chat_messages_keep_tools_reasoning_and_media_placeholders() {
    let request = json!({
        "messages": [
            {
                "role": "assistant",
                "reasoning_content": "think",
                "tool_calls": [{
                    "id": "call_1",
                    "function": {
                        "name": "exec_command",
                        "arguments": "{\"cmd\":\"ls\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": "call_1",
                "content": [
                    { "type": "text", "text": "result" },
                    {
                        "type": "image_url",
                        "image_url": { "url": "data:image/png;base64,c2VjcmV0" }
                    }
                ]
            }
        ]
    });
    let request_events = conversation_events_from_chat_request(&request);
    assert!(request_events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::Reasoning(reasoning) if reasoning.content.as_deref() == Some("think")
    )));
    assert!(request_events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::ToolCall(tool_call)
            if tool_call.tool_call_id == "call_1" && tool_call.name == "exec_command"
    )));
    assert!(request_events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::ToolResult(tool_result)
            if tool_result.tool_call_id == "call_1"
                && tool_result.content
                    == json!([
                        { "type": "text", "text": "result" },
                        { "type": "image_url", "omitted": true }
                    ])
    )));
    let serialized = serde_json::to_string(&request_events).expect("serialize chat request");
    assert!(!serialized.contains("c2VjcmV0"));

    let response = json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "answer",
                "refusal": "refused"
            }
        }],
        "usage": { "prompt_tokens": 3, "completion_tokens": 4 }
    });
    let response_events = conversation_events_from_chat_response(&response);
    assert!(response_events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::Message(message)
            if message.content
                == json!([
                    { "type": "text", "text": "answer" },
                    { "type": "refusal", "text": "refused" }
                ])
    )));
    assert!(response_events
        .iter()
        .any(|event| matches!(&event.body, ContentEventBody::Usage(_))));
}

#[test]
fn image_and_stream_events_store_only_attachment_placeholders() {
    let request = json!({ "prompt": "draw a cat" });
    let request_events = conversation_events_from_image_request(&request);
    assert!(matches!(
        &request_events[0].body,
        ContentEventBody::Message(message) if message.content == json!("draw a cat")
    ));
    let response_events = conversation_events_from_image_response();
    assert!(matches!(
        &response_events[0].body,
        ContentEventBody::Message(message)
            if message.content
                == json!([{ "type": "output_image", "omitted": true }])
    ));

    let mut capture = RawStreamContentCapture::new(RawStreamProtocol::ImageGeneration);
    capture.observe_chunk(
        br#"data: {"type":"image_generation.completed","result":"data:image/png;base64,c2VjcmV0"}

"#,
    );
    capture.finish();
    let capture_events = conversation_events_from_stream_capture(&capture);
    let serialized = serde_json::to_string(&capture_events).expect("serialize image capture");
    assert!(!serialized.contains("c2VjcmV0"));

    let snapshot = StreamStatsSnapshot {
        output_text: "stream answer".to_string(),
        reasoning_text: "stream reasoning".to_string(),
        input_tokens: Some(3),
        output_tokens: Some(4),
        ..Default::default()
    };
    let snapshot_events = conversation_events_from_stream_snapshot(&snapshot);
    assert!(snapshot_events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::Message(message) if message.content == json!("stream answer")
    )));
    assert!(snapshot_events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::Reasoning(reasoning)
            if reasoning.content.as_deref() == Some("stream reasoning")
    )));
}

#[test]
fn internal_response_keeps_message_tool_reasoning_and_usage() {
    let response = InternalResponse {
        id: "resp_1".to_string(),
        model: "model".to_string(),
        output: vec![
            InternalOutputItem::Message {
                id: "msg_1".to_string(),
                role: InternalRole::Assistant,
                content: vec![InternalContentPart::Text("answer".to_string())],
            },
            InternalOutputItem::FunctionToolCall {
                id: "call_1".to_string(),
                name: "exec_command".to_string(),
                arguments: "{\"cmd\":\"ls\"}".to_string(),
                reasoning_content: Some("inspect first".to_string()),
            },
        ],
        usage: Some(InternalUsage {
            input_tokens: Some(3),
            output_tokens: Some(4),
            reasoning_tokens: Some(2),
            cache_read_tokens: Some(1),
            cache_write_tokens: None,
        }),
    };

    let events = conversation_events_from_internal_response(&response);
    assert!(events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::Message(message)
            if message.content == json!("answer") && event.item_id.as_deref() == Some("msg_1")
    )));
    assert!(events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::Reasoning(reasoning)
            if reasoning.content.as_deref() == Some("inspect first")
    )));
    assert!(events.iter().any(|event| matches!(
        &event.body,
        ContentEventBody::ToolCall(tool_call) if tool_call.tool_call_id == "call_1"
    )));
    assert!(events
        .iter()
        .any(|event| matches!(&event.body, ContentEventBody::Usage(_))));
}

#[test]
fn tool_arguments_drop_secrets_and_omit_attachment_payloads() {
    let payload = json!({
        "input": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "read_file",
            "arguments": {
                "authorization": "Bearer sk-live-secret",
                "image": {
                    "type": "input_image",
                    "image_url": "data:image/png;base64,c2VjcmV0",
                    "filename": "private.png",
                    "sha256": "abc123"
                },
                "file_id": "file_private",
                "data": "data:application/octet-stream;base64,YWJj",
                "path": "Cargo.toml"
            }
        }]
    });
    let input_items = payload["input"].as_array().expect("input items");

    let events = conversation_events_from_responses_request(&payload, input_items);
    let ContentEventBody::ToolCall(tool_call) = &events[0].body else {
        panic!("expected tool call");
    };
    assert_eq!(
        tool_call.arguments,
        json!({
            "image": { "type": "input_image", "omitted": true },
            "file_id": "file_private",
            "data": { "type": "binary", "omitted": true },
            "path": "Cargo.toml"
        })
    );
}

#[test]
fn tool_arguments_keep_non_attachment_metadata_and_long_text() {
    let long_text = "a".repeat(512);
    let payload = json!({
        "input": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "read_file",
            "arguments": {
                "filename": "Cargo.toml",
                "hash": "sha256-placeholder",
                "size_bytes": 42,
                "note": long_text,
                "authorization": "Bearer sk-live-secret"
            }
        }]
    });
    let input_items = payload["input"].as_array().expect("input items");

    let events = conversation_events_from_responses_request(&payload, input_items);
    let ContentEventBody::ToolCall(tool_call) = &events[0].body else {
        panic!("expected tool call");
    };
    assert_eq!(tool_call.arguments["filename"], json!("Cargo.toml"));
    assert_eq!(tool_call.arguments["hash"], json!("sha256-placeholder"));
    assert_eq!(tool_call.arguments["size_bytes"], json!(42));
    assert_eq!(tool_call.arguments["note"], json!(long_text));
    assert!(tool_call.arguments.get("authorization").is_none());
}

#[test]
fn tool_arguments_omit_attachment_payloads_by_field() {
    let payload = json!({
        "input": [{
            "type": "function_call",
            "call_id": "call_1",
            "name": "read_file",
            "arguments": {
                "file_data": "raw file bytes",
                "b64_json": "QUJD",
                "url": "data:image/png;base64,c2VjcmV0",
                "path": "Cargo.toml"
            }
        }]
    });
    let input_items = payload["input"].as_array().expect("input items");

    let events = conversation_events_from_responses_request(&payload, input_items);
    let ContentEventBody::ToolCall(tool_call) = &events[0].body else {
        panic!("expected tool call");
    };
    assert_eq!(
        tool_call.arguments["file_data"],
        json!({ "type": "binary", "omitted": true })
    );
    assert_eq!(
        tool_call.arguments["b64_json"],
        json!({ "type": "binary", "omitted": true })
    );
    assert_eq!(
        tool_call.arguments["url"],
        json!({ "type": "binary", "omitted": true })
    );
    assert_eq!(tool_call.arguments["path"], json!("Cargo.toml"));
}
