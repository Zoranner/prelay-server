mod decode;
mod encode;
mod events;
mod pipeline;
pub(crate) mod sse;

use axum::body::Bytes;
use futures::Stream;

#[cfg(test)]
pub(crate) use events::InternalFinishReason;
pub(crate) use events::StreamStatsSnapshot;
pub(crate) use events::{InternalStreamEvent, StreamUsage};
pub(crate) use pipeline::SharedStreamStats;

use self::{
    decode::{
        anthropic::AnthropicMessagesToResponsesSseDecoder,
        chat::ChatToResponsesSseDecoder,
        responses::{NativeResponsesSseStatsDecoder, ResponsesSseAnthropicMessagesSseDecoder},
    },
    encode::anthropic::AnthropicMessagesSseDecoder,
    pipeline::{map_response_stream, map_response_stream_with_stats},
};

#[allow(dead_code)]
pub fn chat_sse_response_to_responses_sse(
    response: reqwest::Response,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    map_response_stream(response, ChatToResponsesSseDecoder::default())
}

#[allow(dead_code)]
pub fn chat_sse_response_to_anthropic_messages_sse(
    response: reqwest::Response,
    model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    map_response_stream(response, AnthropicMessagesSseDecoder::new(model))
}

#[allow(dead_code)]
pub fn responses_sse_response_to_anthropic_messages_sse(
    response: reqwest::Response,
    model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    map_response_stream(
        response,
        ResponsesSseAnthropicMessagesSseDecoder::new(model),
    )
}

#[allow(dead_code)]
pub fn anthropic_messages_sse_response_to_responses_sse(
    response: reqwest::Response,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    map_response_stream(response, AnthropicMessagesToResponsesSseDecoder::default())
}

pub(crate) fn chat_sse_response_to_responses_sse_with_stats(
    response: reqwest::Response,
) -> (
    impl Stream<Item = Result<Bytes, std::io::Error>>,
    SharedStreamStats,
) {
    map_response_stream_with_stats(response, ChatToResponsesSseDecoder::default())
}

pub(crate) fn chat_sse_response_to_anthropic_messages_sse_with_stats(
    response: reqwest::Response,
    model: String,
) -> (
    impl Stream<Item = Result<Bytes, std::io::Error>>,
    SharedStreamStats,
) {
    map_response_stream_with_stats(response, AnthropicMessagesSseDecoder::new(model))
}

pub(crate) fn responses_sse_response_to_anthropic_messages_sse_with_stats(
    response: reqwest::Response,
    model: String,
) -> (
    impl Stream<Item = Result<Bytes, std::io::Error>>,
    SharedStreamStats,
) {
    map_response_stream_with_stats(
        response,
        ResponsesSseAnthropicMessagesSseDecoder::new(model),
    )
}

pub(crate) fn native_responses_sse_with_stats(
    response: reqwest::Response,
) -> (
    impl Stream<Item = Result<Bytes, std::io::Error>>,
    SharedStreamStats,
) {
    map_response_stream_with_stats(response, NativeResponsesSseStatsDecoder::default())
}

pub(crate) fn anthropic_messages_sse_response_to_responses_sse_with_stats(
    response: reqwest::Response,
) -> (
    impl Stream<Item = Result<Bytes, std::io::Error>>,
    SharedStreamStats,
) {
    map_response_stream_with_stats(response, AnthropicMessagesToResponsesSseDecoder::default())
}

pub fn responses_text_delta_sse(delta: &str) -> Bytes {
    encode::responses::responses_text_delta_sse(delta)
}

pub fn responses_completed_sse() -> Bytes {
    encode::responses::responses_completed_sse()
}
