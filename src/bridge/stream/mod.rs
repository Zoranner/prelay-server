mod decode;
mod encode;
mod events;
mod pipeline;
pub(crate) mod sse;
mod verify;

use axum::body::Bytes;
use futures::Stream;

use crate::error::AppError;

pub(crate) use encode::responses::responses_completed_sse_with_usage;
#[cfg(test)]
pub(crate) use events::InternalFinishReason;
pub(crate) use events::StreamStatsSnapshot;
pub(crate) use events::{InternalStreamEvent, StreamUsage};
pub(crate) use pipeline::{upstream_bytes_stream, with_sse_keepalive, SharedStreamStats};
pub(crate) use verify::verify_upstream_sse_head;

use self::{
    decode::{
        chat::{ChatSseStatsDecoder, ChatToResponsesSseDecoder},
        responses::NativeResponsesSseStatsDecoder,
    },
    pipeline::{map_response_stream, map_response_stream_with_stats_and_head},
};

#[allow(dead_code)]
pub fn chat_sse_response_to_responses_sse(
    response: reqwest::Response,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    map_response_stream(
        response,
        ChatToResponsesSseDecoder::new(format!("resp_{}", uuid::Uuid::new_v4().simple())),
    )
}

pub(crate) async fn chat_sse_response_to_responses_sse_with_stats(
    mut response: reqwest::Response,
    response_id: String,
) -> Result<
    (
        impl Stream<Item = Result<Bytes, std::io::Error>>,
        SharedStreamStats,
    ),
    AppError,
> {
    let head = verify_upstream_sse_head(&mut response).await?;
    Ok(map_response_stream_with_stats_and_head(
        response,
        head,
        ChatToResponsesSseDecoder::new(response_id),
    ))
}

pub(crate) async fn native_responses_sse_with_stats(
    mut response: reqwest::Response,
) -> Result<
    (
        impl Stream<Item = Result<Bytes, std::io::Error>>,
        SharedStreamStats,
    ),
    AppError,
> {
    let head = verify_upstream_sse_head(&mut response).await?;
    Ok(map_response_stream_with_stats_and_head(
        response,
        head,
        NativeResponsesSseStatsDecoder::default(),
    ))
}

pub(crate) async fn native_chat_sse_with_stats(
    mut response: reqwest::Response,
) -> Result<
    (
        impl Stream<Item = Result<Bytes, std::io::Error>>,
        SharedStreamStats,
    ),
    AppError,
> {
    let head = verify_upstream_sse_head(&mut response).await?;
    Ok(map_response_stream_with_stats_and_head(
        response,
        head,
        ChatSseStatsDecoder::default(),
    ))
}

pub fn responses_text_delta_sse(delta: &str) -> Bytes {
    encode::responses::responses_text_delta_sse(delta)
}

pub fn responses_completed_sse() -> Bytes {
    encode::responses::responses_completed_sse()
}
