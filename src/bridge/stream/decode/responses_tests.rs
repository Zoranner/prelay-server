use super::{decode_responses_usage, NativeResponsesSseStatsDecoder};
use std::sync::{Arc, Mutex};

use crate::bridge::stream::{pipeline::ByteStreamDecoder, StreamStatsSnapshot};

#[test]
fn records_the_upstream_response_id_from_native_responses_sse() {
    let mut decoder = NativeResponsesSseStatsDecoder::default();
    let stats = Arc::new(Mutex::new(StreamStatsSnapshot::default()));
    decoder.set_stats(stats.clone());

    decoder.push_chunk(
        b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_upstream\"}}\n\n\
          event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_upstream\",\"usage\":{\"input_tokens\":3,\"output_tokens\":5}}}\n\n",
    );

    let stats = stats.lock().expect("stats lock");
    assert_eq!(stats.response_id.as_deref(), Some("resp_upstream"));
    assert!(stats.completed);
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
