use std::{
    fmt::{self, Write as _},
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use futures::{stream, StreamExt, TryStreamExt};
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer, Registry};

use super::{
    persistence::log_stream_storage_failure, record_first_chunk, record_stream,
    state::record_stream_with_log_id,
};
use crate::{
    bridge::stream::StreamStatsSnapshot,
    observability::request_metadata::build_request_metadata,
    stats::RequestLogInsert,
    storage::{Storage, StorageError},
};

#[tokio::test]
async fn record_first_chunk_updates_completed_metadata_on_eof() {
    let (storage, identity_id) = test_storage().await;
    let metadata_json = test_metadata();
    let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))]);

    let chunks = record_first_chunk(
        storage.clone(),
        identity_id.clone(),
        stream,
        test_log(metadata_json),
        std::time::Instant::now(),
    )
    .try_collect::<Vec<_>>()
    .await
    .expect("collect stream");

    assert_eq!(chunks, vec![Bytes::from_static(b"hello")]);

    let logs = storage
        .list_request_logs(&identity_id, 10)
        .await
        .expect("load stream log");
    let metadata: serde_json::Value =
        serde_json::from_str(logs[0].metadata_json.as_deref().expect("metadata"))
            .expect("parse metadata");

    assert_eq!(logs.len(), 1);
    assert_eq!(metadata["stream"]["empty"], false);
    assert_eq!(metadata["stream"]["completed"], true);
    assert_eq!(metadata["stream"]["final_usage_seen"], false);
}

#[tokio::test]
async fn record_stream_updates_usage_and_tool_count_without_normal_stream_metadata() {
    let (storage, identity_id) = test_storage().await;
    let metadata_json = test_metadata();
    let stats = Arc::new(Mutex::new(StreamStatsSnapshot {
        input_tokens: Some(11),
        output_tokens: Some(7),
        total_tokens: Some(18),
        cache_read_tokens: Some(3),
        cache_write_tokens: Some(2),
        tool_call_count: 2,
        completed: true,
        final_usage_seen: true,
    }));
    let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))]);

    record_stream(
        storage.clone(),
        identity_id.clone(),
        stream,
        test_log(metadata_json),
        std::time::Instant::now(),
        stats,
    )
    .try_collect::<Vec<_>>()
    .await
    .expect("collect stream");

    let logs = storage
        .list_request_logs(&identity_id, 10)
        .await
        .expect("load stream log");
    let row = &logs[0];
    let metadata: serde_json::Value =
        serde_json::from_str(row.metadata_json.as_deref().expect("metadata"))
            .expect("parse metadata");

    assert_eq!(logs.len(), 1);
    assert_eq!(row.input_tokens, Some(11));
    assert_eq!(row.output_tokens, Some(7));
    assert_eq!(row.cache_read_tokens, Some(3));
    assert_eq!(row.cache_write_tokens, Some(2));
    assert!(metadata["stream"].is_null());
    assert_eq!(
        metadata["diagnostics"][0]["code"],
        "responses.content.non_text"
    );
}

#[tokio::test]
async fn record_stream_persists_final_usage_before_eof() {
    let (storage, identity_id) = test_storage().await;
    let stats = Arc::new(Mutex::new(StreamStatsSnapshot {
        input_tokens: Some(11),
        output_tokens: Some(7),
        total_tokens: Some(18),
        cache_read_tokens: Some(3),
        cache_write_tokens: Some(2),
        tool_call_count: 0,
        completed: true,
        final_usage_seen: true,
    }));
    let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))])
        .chain(stream::pending());
    let mut output = Box::pin(record_stream(
        storage.clone(),
        identity_id.clone(),
        stream,
        test_log(test_metadata()),
        std::time::Instant::now(),
        stats,
    ));

    assert_eq!(
        output
            .next()
            .await
            .expect("first chunk")
            .expect("stream chunk"),
        Bytes::from_static(b"hello")
    );

    let logs = storage
        .list_request_logs(&identity_id, 10)
        .await
        .expect("load stream log");

    assert_eq!(logs[0].input_tokens, Some(11));
    assert_eq!(logs[0].output_tokens, Some(7));
}

#[tokio::test]
async fn record_stream_does_not_update_existing_row_when_first_insert_fails() {
    let (storage, identity_id) = test_storage().await;
    storage
        .insert_request_log_with_id(
            &identity_id,
            "duplicate-stream-log".to_string(),
            test_log(test_metadata()),
        )
        .await
        .expect("insert existing log");
    let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))]);

    record_stream_with_log_id(
        storage.clone(),
        identity_id.clone(),
        stream,
        test_log(test_metadata()),
        std::time::Instant::now(),
        None,
        "duplicate-stream-log".to_string(),
    )
    .try_collect::<Vec<_>>()
    .await
    .expect("collect stream");

    let logs = storage
        .list_request_logs(&identity_id, 10)
        .await
        .expect("load duplicate log");
    let row = logs
        .iter()
        .find(|log| log.id == "duplicate-stream-log")
        .expect("load duplicate log");
    let metadata: serde_json::Value =
        serde_json::from_str(row.metadata_json.as_deref().expect("metadata"))
            .expect("parse metadata");

    assert_eq!(logs.len(), 1);
    assert_eq!(metadata["stream"]["completed"], serde_json::Value::Null);
}

async fn test_storage() -> (Storage, String) {
    let storage = crate::test_support::test_state().await.storage;
    let identity = storage
        .register_identity(
            "machine-1",
            "S-1-5-21-1",
            &crate::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    (storage, identity.identity_id)
}

fn test_metadata() -> String {
    build_request_metadata(vec![crate::bridge::diagnostics::BridgeDiagnostic::new(
        "responses",
        "/input/0/content",
        crate::bridge::diagnostics::DiagnosticAction::Textified,
        crate::bridge::diagnostics::DiagnosticSeverity::Info,
        "responses.content.non_text",
        "非文本 content 已转为 JSON 字符串",
        Some("object".to_string()),
    )])
    .expect("build metadata")
    .expect("metadata for diagnostic")
}

fn test_log(metadata_json: String) -> RequestLogInsert {
    RequestLogInsert {
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
        upstream_request_id: Some("req_123".to_string()),
        metadata_json: Some(metadata_json),
    }
}

#[derive(Clone)]
struct EventCapture {
    events: Arc<Mutex<Vec<String>>>,
}

impl<S> Layer<S> for EventCapture
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = FieldVisitor(String::new());
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("lock captured tracing events")
            .push(format!("{} {}", event.metadata().name(), visitor.0));
    }
}

struct FieldVisitor(String);

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let _ = write!(self.0, "{}={value:?};", field.name());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let _ = write!(self.0, "{}={value};", field.name());
    }
}

#[test]
fn stream_log_write_failure_excludes_error_details() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Registry::default().with(EventCapture {
        events: Arc::clone(&events),
    });
    let error = StorageError::ValidationFailed(
        "storage error: SELECT provider_key WHERE device_credential = 'device-secret'".to_string(),
    );

    tracing::subscriber::with_default(subscriber, || {
        log_stream_storage_failure("insert", &error);
    });

    let events = events.lock().expect("lock captured tracing events");
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("operation=insert"));
    assert!(events[0].contains("failure_kind=stream_log_storage"));
    assert!(!events[0].contains("provider_key"));
    assert!(!events[0].contains("device_credential"));
    assert!(!events[0].contains("device-secret"));
}
