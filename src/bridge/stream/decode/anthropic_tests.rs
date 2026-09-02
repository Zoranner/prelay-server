use super::AnthropicMessagesToResponsesSseDecoder;
use crate::bridge::stream::{
    pipeline::ByteStreamDecoder, responses_completed_sse, responses_text_delta_sse,
    InternalFinishReason, InternalStreamEvent,
};

#[test]
fn decodes_anthropic_messages_sse_text_delta_to_responses_sse() {
    let mut decoder = AnthropicMessagesToResponsesSseDecoder::default();

    let chunks = decoder.push_chunk(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
        );

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], responses_text_delta_sse("hel"));
}

#[test]
fn decodes_anthropic_message_start_usage() {
    let events = super::decode_anthropic_messages_sse_event(
        r#"{"type":"message_start","message":{"usage":{"input_tokens":7,"cache_read_input_tokens":3,"cache_creation_input_tokens":2}}}"#,
    );

    assert!(events.iter().any(|event| {
        matches!(
            event,
            InternalStreamEvent::Usage(usage)
                if usage.input_tokens == Some(7)
                    && usage.cache_read_tokens == Some(3)
                    && usage.cache_write_tokens == Some(2)
        )
    }));
}

#[test]
fn does_not_turn_anthropic_text_block_stop_into_responses_function_call() {
    let mut decoder = AnthropicMessagesToResponsesSseDecoder::default();

    let chunks = decoder.push_chunk(
            br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":3,"output_tokens":5}}

"#,
        );
    let output = chunks
        .iter()
        .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
        .collect::<String>();

    assert!(output.contains("event: response.output_text.delta"));
    assert!(output.contains("event: response.completed"));
    assert!(!output.contains("event: response.output_item.added"));
    assert!(!output.contains(r#""type":"function_call""#));
    assert!(!output.contains("event: response.function_call_arguments.done"));
    assert!(!output.contains("event: response.output_item.done"));
}

#[test]
fn maps_anthropic_message_delta_stop_reason_to_internal_finish_reason() {
    let tool_use_events = super::decode_anthropic_messages_sse_event(
        r#"{"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null}}"#,
    );
    assert!(tool_use_events.iter().any(|event| {
        matches!(
            event,
            InternalStreamEvent::Finished(InternalFinishReason::ToolUse)
        )
    }));

    let length_events = super::decode_anthropic_messages_sse_event(
        r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens","stop_sequence":null}}"#,
    );
    assert!(length_events.iter().any(|event| {
        matches!(
            event,
            InternalStreamEvent::Finished(InternalFinishReason::Length)
        )
    }));

    let stop_events = super::decode_anthropic_messages_sse_event(
        r#"{"type":"message_delta","delta":{"stop_reason":"stop_sequence","stop_sequence":"\n"}}"#,
    );
    assert!(stop_events.iter().any(|event| {
        matches!(
            event,
            InternalStreamEvent::Finished(InternalFinishReason::Stop)
        )
    }));
}

#[test]
fn decodes_anthropic_messages_sse_stop_to_responses_completed() {
    let mut decoder = AnthropicMessagesToResponsesSseDecoder::default();

    let chunks = decoder.push_chunk(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n\
              event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0], responses_text_delta_sse("hel"));
    assert_eq!(chunks[1], responses_completed_sse());
}

#[test]
fn decodes_anthropic_messages_tool_use_to_responses_function_call_sse() {
    let mut decoder = AnthropicMessagesToResponsesSseDecoder::default();

    let chunks = decoder.push_chunk(
            br#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_weather","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\":\"Par"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"is\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"input_tokens":3,"output_tokens":5}}

event: message_stop
data: {"type":"message_stop"}

"#,
        );
    let output = chunks
        .iter()
        .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
        .collect::<String>();

    assert!(output.contains("event: response.output_item.added"));
    assert!(output.contains(r#""type":"function_call""#));
    assert!(output.contains(r#""id":"toolu_1""#));
    assert!(output.contains(r#""call_id":"toolu_1""#));
    assert!(output.contains(r#""name":"get_weather""#));
    assert!(output.contains("event: response.function_call_arguments.delta"));
    assert!(output.contains(r#""delta":"{\"city\":\"Par""#));
    assert!(output.contains(r#""delta":"is\"}""#));
    assert!(output.contains("event: response.function_call_arguments.done"));
    assert!(output.contains(r#""arguments":"{\"city\":\"Paris\"}""#));
    assert!(output.contains("event: response.output_item.done"));
    assert!(output.contains("event: response.completed"));
}

#[test]
fn ignores_unknown_anthropic_messages_sse_event_and_continues() {
    let mut decoder = AnthropicMessagesToResponsesSseDecoder::default();

    let chunks = decoder.push_chunk(
            b"event: ping\ndata: {\"type\":\"ping\"}\n\n\
              event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
        );

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], responses_text_delta_sse("hel"));
}
