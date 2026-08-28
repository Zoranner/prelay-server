use super::AnthropicMessagesSseDecoder;
use std::sync::{Arc, Mutex};

use crate::bridge::stream::{pipeline::ByteStreamDecoder, StreamStatsSnapshot};

#[test]
fn decodes_chat_sse_text_delta_to_anthropic_messages_sse() {
    let mut decoder = AnthropicMessagesSseDecoder::new("deepseek-chat".to_string());

    let chunks = decoder.push_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n");

    assert_eq!(chunks.len(), 1);
    let chunk = std::str::from_utf8(&chunks[0]).expect("utf8 chunk");
    assert!(chunk.contains("event: message_start"));
    assert!(chunk.contains("event: content_block_start"));
    assert!(chunk.contains("event: content_block_delta"));
    assert!(chunk.contains("\"text\":\"hel\""));
}

#[test]
fn decodes_chat_sse_tool_call_to_anthropic_messages_sse() {
    let mut decoder = AnthropicMessagesSseDecoder::new("deepseek-chat".to_string());

    let chunks = decoder.push_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}]}}]}

data: [DONE]

"#,
        );

    assert_eq!(chunks.len(), 3);
    let output = chunks
        .iter()
        .map(|chunk| std::str::from_utf8(chunk).expect("utf8 chunk"))
        .collect::<String>();
    assert!(output.contains("event: message_start"));
    assert!(output.contains("event: content_block_start"));
    assert!(output.contains(r#""type":"tool_use""#));
    assert!(output.contains(r#""id":"call_1""#));
    assert!(output.contains(r#""name":"get_weather""#));
    assert!(output.contains(r#""input":{}"#));
    assert!(output.contains("event: content_block_delta"));
    assert!(output.contains(r#""type":"input_json_delta""#));
    assert!(output.contains(r#""partial_json":"{\"city\":\"Paris\"}""#));
    assert!(output.contains("event: content_block_stop"));
    assert!(output.contains("event: message_delta"));
    assert!(output.contains(r#""stop_reason":"tool_use""#));
    assert!(output.contains("event: message_stop"));
}

#[test]
fn records_tool_call_done_when_finish_message_closes_open_tool_block() {
    let mut decoder = AnthropicMessagesSseDecoder::new("deepseek-chat".to_string());
    let stats = Arc::new(Mutex::new(StreamStatsSnapshot::default()));
    decoder.set_stats(stats.clone());

    let chunks = decoder.push_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":"{}"}}]}}]}

data: [DONE]

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
