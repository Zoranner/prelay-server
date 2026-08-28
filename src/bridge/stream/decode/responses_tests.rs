use super::{decode_responses_usage, ResponsesSseAnthropicMessagesSseDecoder};
use std::sync::{Arc, Mutex};

use crate::bridge::stream::{pipeline::ByteStreamDecoder, StreamStatsSnapshot};

#[test]
fn decodes_responses_sse_text_delta_to_anthropic_messages_sse() {
    let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());

    let chunks =
        decoder.push_chunk(b"event: response.output_text.delta\ndata: {\"delta\":\"hel\"}\n\n");

    assert_eq!(chunks.len(), 1);
    let chunk = std::str::from_utf8(&chunks[0]).expect("utf8 chunk");
    assert!(chunk.contains("event: message_start"));
    assert!(chunk.contains("event: content_block_start"));
    assert!(chunk.contains("event: content_block_delta"));
    assert!(chunk.contains("\"text\":\"hel\""));
}

#[test]
fn decodes_responses_sse_completion_to_anthropic_messages_stop() {
    let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());

    let chunks = decoder.push_chunk(
        b"event: response.output_text.delta\ndata: {\"delta\":\"hel\"}\n\n\
              event: response.completed\ndata: {}\n\n",
    );
    let output = chunks
        .iter()
        .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
        .collect::<String>();

    assert!(output.contains("event: content_block_delta"));
    assert!(output.contains("event: content_block_stop"));
    assert!(output.contains("event: message_delta"));
    assert!(output.contains("event: message_stop"));
}

#[test]
fn decodes_responses_sse_function_call_to_anthropic_messages_tool_use() {
    let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());

    let chunks = decoder.push_chunk(
            br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather","arguments":""}}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","call_id":"call_1","delta":"{\"city\":\"Par"}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","call_id":"call_1","delta":"is\"}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","call_id":"call_1","name":"get_weather","arguments":"{\"city\":\"Paris\"}"}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}

event: response.completed
data: {"type":"response.completed","response":{"usage":{"input_tokens":3,"output_tokens":5,"total_tokens":8}}}

"#,
        );
    let output = chunks
        .iter()
        .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
        .collect::<String>();

    assert!(output.contains("event: message_start"));
    assert!(output.contains("event: content_block_start"));
    assert!(output.contains(r#""type":"tool_use""#));
    assert!(output.contains(r#""id":"call_1""#));
    assert!(output.contains(r#""name":"get_weather""#));
    assert!(output.contains("event: content_block_delta"));
    assert!(output.contains(r#""type":"input_json_delta""#));
    assert!(output.contains(r#""partial_json":"{\"city\":\"Par""#));
    assert!(output.contains(r#""partial_json":"is\"}""#));
    assert!(output.contains("event: content_block_stop"));
    assert!(output.contains(r#""stop_reason":"tool_use""#));
    assert!(output.contains("event: message_stop"));
}

#[test]
fn records_responses_function_call_done_once_when_arguments_and_item_done_arrive() {
    let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());
    let stats = Arc::new(Mutex::new(StreamStatsSnapshot::default()));
    decoder.set_stats(stats.clone());

    let chunks = decoder.push_chunk(
            br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather","arguments":""}}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","call_id":"call_1","delta":"{}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","call_id":"call_1","name":"get_weather","arguments":"{}"}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather","arguments":"{}"}}

event: response.completed
data: {"type":"response.completed","response":{"usage":{"input_tokens":3,"output_tokens":5,"total_tokens":8}}}

"#,
        );
    let output = chunks
        .iter()
        .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
        .collect::<String>();
    let block_stop_count = output.matches("event: content_block_stop").count();
    let stats = stats.lock().expect("stats lock");

    assert_eq!(block_stop_count, 1);
    assert_eq!(stats.tool_call_count, 1);
    assert!(stats.completed);
}

#[test]
fn ignores_unknown_responses_sse_event_and_continues() {
    let mut decoder = ResponsesSseAnthropicMessagesSseDecoder::new("gpt-4.1".to_string());

    let chunks = decoder.push_chunk(
        b"event: response.unexpected\ndata: {\"type\":\"response.unexpected\"}\n\n\
              event: response.output_text.delta\ndata: {\"delta\":\"hel\"}\n\n",
    );
    let output = chunks
        .iter()
        .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
        .collect::<String>();

    assert!(output.contains("event: content_block_delta"));
    assert!(output.contains("\"text\":\"hel\""));
}

#[test]
fn decodes_openai_named_usage_from_completed_responses_event() {
    let usage = decode_responses_usage(
        r#"{"response":{"usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}}"#,
    )
    .expect("usage");

    assert_eq!(usage.input_tokens, Some(11));
    assert_eq!(usage.output_tokens, Some(7));
    assert_eq!(usage.total_tokens, Some(18));
}
