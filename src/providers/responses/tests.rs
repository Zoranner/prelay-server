use serde_json::json;

use super::{
    decode_responses_response, encode_native_responses_request_body, encode_responses_request,
};
use crate::bridge::internal::{
    InternalContentPart, InternalImagePart, InternalMessage, InternalRequest, InternalRole,
    InternalTool, InternalToolCall,
};

#[test]
fn native_request_body_replaces_only_the_model_field() {
    let payload = json!({
        "model": "gpt-6.1-sol",
        "stream": true,
        "input": "hello",
        "metadata": { "nested": [1, 2, 3] }
    });
    let mut expected_payload = payload.clone();
    expected_payload["model"] = json!("gpt-6.1-sol-upstream");

    let body = encode_native_responses_request_body(&payload, "gpt-6.1-sol-upstream");

    assert_eq!(
        body,
        serde_json::to_vec(&expected_payload).expect("expected body")
    );
    assert_eq!(payload["model"], "gpt-6.1-sol");
}

#[test]
fn native_request_body_adds_model_when_absent() {
    let body = encode_native_responses_request_body(&json!({ "input": "hello" }), "k3");

    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("body is json")["model"],
        "k3"
    );
}

#[test]
fn encodes_history_images_and_tool_calls_without_losing_them() {
    let encoded = encode_responses_request(&InternalRequest {
        model: "deepseek-flash".to_string(),
        stream: true,
        max_tokens: None,
        max_completion_tokens: None,
        previous_response_id: Some("resp_previous".to_string()),
        reasoning_requested: false,
        tool_choice_requested: false,
        structured_output_requested: false,
        parallel_tool_calls_requested: false,
        streaming_usage_requested: false,
        instructions: None,
        store: true,
        reasoning: None,
        tool_choice: None,
        parallel_tool_calls: None,
        text: None,
        tools: Vec::new(),
        messages: vec![
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
            InternalMessage {
                role: InternalRole::Tool,
                content: vec![
                    InternalContentPart::Text("rendered".to_string()),
                    InternalContentPart::Image(InternalImagePart {
                        image_url: Some(json!("data:image/png;base64,BBB")),
                        file_id: None,
                        detail: None,
                    }),
                ],
                tool_call_id: Some("call_1".to_string()),
                tool_calls: Vec::new(),
                reasoning_content: None,
            },
        ],
    });

    assert_eq!(encoded["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(encoded["input"][0]["content"][1]["type"], "input_image");
    assert_eq!(
        encoded["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,AAA"
    );
    assert_eq!(encoded["input"][1]["type"], "function_call");
    assert_eq!(encoded["input"][1]["call_id"], "call_1");
    assert_eq!(encoded["input"][1]["name"], "read_file");
    assert_eq!(encoded["input"][2]["type"], "function_call_output");
    assert_eq!(encoded["input"][2]["output"][0]["type"], "input_text");
    assert_eq!(encoded["input"][2]["output"][1]["type"], "input_image");
    assert_eq!(
        encoded["input"][2]["output"][1]["image_url"],
        "data:image/png;base64,BBB"
    );
}

#[test]
fn encodes_internal_request_to_responses_shape() {
    let encoded = encode_responses_request(&InternalRequest {
        model: "gpt-4.1".to_string(),
        stream: false,
        max_tokens: Some(128),
        max_completion_tokens: None,
        previous_response_id: None,
        reasoning_requested: false,
        tool_choice_requested: false,
        structured_output_requested: false,
        parallel_tool_calls_requested: false,
        streaming_usage_requested: false,
        instructions: Some("Be concise.".to_string()),
        store: false,
        reasoning: Some(json!({ "effort": "high" })),
        tool_choice: Some(json!("required")),
        parallel_tool_calls: Some(true),
        text: Some(json!({ "format": { "type": "json_object" } })),
        tools: Vec::new(),
        messages: vec![InternalMessage {
            role: InternalRole::User,
            content: vec![InternalContentPart::Text("hello".to_string())],
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        }],
    });

    assert_eq!(encoded["model"], "gpt-4.1");
    assert_eq!(encoded["stream"], false);
    assert_eq!(encoded["max_output_tokens"], 128);
    assert_eq!(encoded["input"][0]["role"], "user");
    assert_eq!(encoded["input"][0]["content"], "hello");
    assert_eq!(encoded["instructions"], "Be concise.");
    assert_eq!(encoded["store"], false);
    assert_eq!(encoded["reasoning"]["effort"], "high");
    assert_eq!(encoded["tool_choice"], "required");
    assert_eq!(encoded["parallel_tool_calls"], true);
    assert_eq!(encoded["text"]["format"]["type"], "json_object");
}

#[test]
fn encodes_internal_tools_to_responses_tools() {
    let encoded = encode_responses_request(&InternalRequest {
        model: "gpt-4.1".to_string(),
        stream: false,
        max_tokens: None,
        max_completion_tokens: None,
        previous_response_id: None,
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
        tools: vec![InternalTool {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        }],
        messages: vec![InternalMessage {
            role: InternalRole::User,
            content: vec![InternalContentPart::Text("read".to_string())],
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
        }],
    });

    assert_eq!(encoded["tools"][0]["type"], "function");
    assert_eq!(encoded["tools"][0]["name"], "read_file");
    assert_eq!(encoded["tools"][0]["description"], "Read a file");
    assert_eq!(
        encoded["tools"][0]["parameters"]["properties"]["path"]["type"],
        "string"
    );
}

#[test]
fn decodes_responses_text_response_to_internal_response() {
    let response = decode_responses_response(json!({
        "id": "resp_123",
        "model": "gpt-4.1",
        "output": [
            {
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "content": [
                    { "type": "output_text", "text": "hello" }
                ]
            }
        ],
        "usage": {
            "input_tokens": 3,
            "output_tokens": 4,
            "input_tokens_details": { "cached_tokens": 2 },
            "cache_creation_input_tokens": 1,
            "output_tokens_details": { "reasoning_tokens": 1 }
        }
    }))
    .expect("decode responses response");

    assert_eq!(response.id, "resp_123");
    assert_eq!(response.model, "gpt-4.1");
    assert_eq!(response.output[0].text_content().as_deref(), Some("hello"));
    let usage = response.usage.expect("usage");
    assert_eq!(usage.reasoning_tokens, Some(1));
    assert_eq!(usage.cache_read_tokens, Some(2));
    assert_eq!(usage.cache_write_tokens, Some(1));
}

#[test]
fn decodes_openai_named_usage_from_responses_response() {
    let response = decode_responses_response(json!({
        "id": "resp_123",
        "model": "gpt-5.6-terra",
        "output": [],
        "usage": {
            "prompt_tokens": 11,
            "completion_tokens": 7
        }
    }))
    .expect("decode responses response");
    let usage = response.usage.expect("usage");

    assert_eq!(usage.input_tokens, Some(11));
    assert_eq!(usage.output_tokens, Some(7));
}

#[test]
fn decodes_only_output_text_content_blocks() {
    let response = decode_responses_response(json!({
        "id": "resp_123",
        "model": "gpt-4.1",
        "output": [
            {
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "content": [
                    { "type": "refusal", "refusal": "no" },
                    { "type": "output_text", "text": "hello" }
                ]
            }
        ]
    }))
    .expect("decode responses response");

    assert_eq!(response.output[0].text_content().as_deref(), Some("hello"));
}

#[test]
fn decodes_responses_function_call_to_internal_response() {
    let response = decode_responses_response(json!({
        "id": "resp_123",
        "model": "gpt-4.1",
        "output": [
            {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"Cargo.toml\"}"
            }
        ]
    }))
    .expect("decode responses response");

    assert_eq!(
        response.output[0].tool_call_name().as_deref(),
        Some("read_file")
    );
    assert_eq!(
        response.output[0].tool_call_arguments().as_deref(),
        Some("{\"path\":\"Cargo.toml\"}")
    );
}
