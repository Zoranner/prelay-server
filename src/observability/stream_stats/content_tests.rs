use futures::{stream, StreamExt, TryStreamExt};

use super::{
    record_first_chunk_with_activity_content,
    state::{prepare_stream_with_log_id, RecordingMode, StreamRecordOptions},
};
use crate::{
    activity::{RawStreamContentCapture, RawStreamProtocol},
    stats::ActivityInsert,
    storage::Storage,
};
use bytes::Bytes;

#[tokio::test]
async fn record_first_chunk_finalizes_content_after_an_upstream_error() {
    let (storage, identity_id) = test_storage().await;
    let first =
        Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n");
    let stream = stream::iter([
        Ok::<_, std::io::Error>(first.clone()),
        Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "upstream disconnected",
        )),
    ]);

    let mut output = Box::pin(
        record_first_chunk_with_activity_content(
            storage.clone(),
            identity_id.clone(),
            stream,
            test_log(),
            std::time::Instant::now(),
            "stream input".to_string(),
            RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
        )
        .await
        .expect("prepare stream"),
    );

    assert_eq!(
        output
            .next()
            .await
            .expect("first chunk")
            .expect("first stream chunk"),
        first
    );
    let error = output
        .next()
        .await
        .expect("upstream error")
        .expect_err("upstream error should be forwarded");
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);

    let activity = storage
        .list_activities(&identity_id, 10)
        .await
        .expect("load failed stream activity")
        .pop()
        .expect("stored failed activity");
    assert_eq!(activity.status, "failed");
    let content = storage
        .find_activity_content(&activity.id)
        .await
        .expect("load failed stream content")
        .expect("stored failed stream content");
    assert_eq!(content.input_text, "stream input");
    assert_eq!(content.output_text, "partial");
    assert_eq!(content.status, "pending");
    assert!(content.is_truncated);
}

#[tokio::test]
async fn stream_activity_is_streaming_before_eof() {
    let (storage, identity_id) = test_storage().await;
    let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))])
        .chain(stream::pending());
    let mut output = Box::pin(
        record_first_chunk_with_activity_content(
            storage.clone(),
            identity_id.clone(),
            stream,
            test_log(),
            std::time::Instant::now(),
            "input".to_string(),
            RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
        )
        .await
        .expect("prepare stream"),
    );

    output
        .next()
        .await
        .expect("first chunk")
        .expect("stream chunk");
    let activity = storage
        .list_activities(&identity_id, 10)
        .await
        .expect("load streaming activity")
        .pop()
        .expect("stored streaming activity");
    assert_eq!(activity.status, "streaming");
    drop(output);
}

#[tokio::test]
async fn record_first_chunk_stores_empty_stream_content_as_pending() {
    let (storage, identity_id) = test_storage().await;
    let stream = stream::empty::<Result<Bytes, std::io::Error>>();

    let output = record_first_chunk_with_activity_content(
        storage.clone(),
        identity_id.clone(),
        stream,
        test_log(),
        std::time::Instant::now(),
        "empty stream input".to_string(),
        RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
    )
    .await
    .expect("prepare empty stream");

    output
        .try_collect::<Vec<_>>()
        .await
        .expect("collect empty stream");

    let activity = storage
        .list_activities(&identity_id, 10)
        .await
        .expect("load empty stream activity")
        .pop()
        .expect("stored empty stream activity");
    assert_eq!(activity.status, "failed");
    let content = storage
        .find_activity_content(&activity.id)
        .await
        .expect("load empty stream content")
        .expect("stored empty stream content");
    assert_eq!(content.input_text, "empty stream input");
    assert_eq!(content.output_text, "");
    assert_eq!(content.status, "pending");
}

#[tokio::test]
async fn empty_stream_keeps_the_recording_activity_id() {
    let (storage, identity_id) = test_storage().await;
    let stream = stream::empty::<Result<Bytes, std::io::Error>>();

    let output = prepare_stream_with_log_id(
        storage.clone(),
        identity_id.clone(),
        stream,
        test_log(),
        std::time::Instant::now(),
        StreamRecordOptions {
            stats: None,
            input_text: "empty stream input".to_string(),
            content_capture: Some(RawStreamContentCapture::new(
                RawStreamProtocol::ChatCompletions,
            )),
            log_id: "empty-stream-id".to_string(),
            mode: RecordingMode::Required,
        },
    )
    .await
    .expect("prepare empty stream");

    output
        .try_collect::<Vec<_>>()
        .await
        .expect("collect empty stream");

    let activity = storage
        .list_activities(&identity_id, 10)
        .await
        .expect("load empty stream activity")
        .pop()
        .expect("stored empty stream activity");
    assert_eq!(activity.id, "empty-stream-id");
}

#[tokio::test]
async fn recording_finishes_content_after_the_client_drops_the_stream() {
    let (storage, identity_id) = test_storage().await;
    let first =
        Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n");
    let stream = stream::iter([
        Ok::<_, std::io::Error>(first.clone()),
        Ok(Bytes::from_static(b"data: [DONE]\n\n")),
    ]);

    let mut output = Box::pin(
        record_first_chunk_with_activity_content(
            storage.clone(),
            identity_id.clone(),
            stream,
            test_log(),
            std::time::Instant::now(),
            "dropped input".to_string(),
            RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions),
        )
        .await
        .expect("prepare stream"),
    );
    assert_eq!(
        output
            .next()
            .await
            .expect("first chunk")
            .expect("first stream chunk"),
        first
    );
    drop(output);

    let content = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if let Some(activity) = storage
                .list_activities(&identity_id, 10)
                .await
                .expect("load dropped stream activity")
                .pop()
            {
                if let Some(content) = storage
                    .find_activity_content(&activity.id)
                    .await
                    .expect("load dropped stream content")
                {
                    if content.status == "pending" {
                        break content;
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("dropped stream should be finalized");

    assert_eq!(content.input_text, "dropped input");
    assert_eq!(content.output_text, "partial");
    assert_eq!(content.status, "pending");
}

async fn test_storage() -> (Storage, String) {
    let storage = crate::test_support::test_state().await.storage;
    let identity = storage
        .register_identity(
            "content-test-machine",
            "S-1-5-21-content",
            &crate::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    (storage, identity.identity_id)
}

fn test_log() -> ActivityInsert {
    ActivityInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: "chat_completions".to_string(),
        endpoint_name: String::new(),
        provider_id: "provider-1".to_string(),
        provider_name: "DeepSeek".to_string(),
        model_requested: "coder".to_string(),
        model_upstream: "deepseek-chat".to_string(),
        status: "success".to_string(),
        http_status: 200,
        error_code: None,
        error_message: None,
        is_streaming: true,
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms: 0,
        upstream_latency_ms: Some(5),
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: Some("req_content_test".to_string()),
    }
}
