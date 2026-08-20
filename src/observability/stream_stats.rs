use std::{pin::Pin, time::Instant};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    bridge::stream::{SharedStreamStats, StreamStatsSnapshot},
    observability::request_metadata::{update_stream_metadata, StreamMetadataUpdate},
    stats::{RequestLogInsert, StreamRequestLogUpdate},
    storage::stats::{insert_with_id, update_stream},
};

pub fn record_first_chunk<S>(
    db: SqlitePool,
    identity_id: String,
    stream: S,
    log: RequestLogInsert,
    started_at: Instant,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_optional_stats(db, identity_id, stream, log, started_at, None)
}

pub fn record_stream<S>(
    db: SqlitePool,
    identity_id: String,
    stream: S,
    log: RequestLogInsert,
    started_at: Instant,
    stats: SharedStreamStats,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_optional_stats(db, identity_id, stream, log, started_at, Some(stats))
}

fn record_stream_with_optional_stats<S>(
    db: SqlitePool,
    identity_id: String,
    stream: S,
    log: RequestLogInsert,
    started_at: Instant,
    stats: Option<SharedStreamStats>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    record_stream_with_log_id(
        db,
        identity_id,
        stream,
        log,
        started_at,
        stats,
        Uuid::new_v4().to_string(),
    )
}

fn record_stream_with_log_id<S>(
    db: SqlitePool,
    identity_id: String,
    stream: S,
    log: RequestLogInsert,
    started_at: Instant,
    stats: Option<SharedStreamStats>,
    log_id: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let base_metadata_json = log.metadata_json.clone();
    let upstream_request_id = log.upstream_request_id.clone();
    let state = StreamRecordState {
        db,
        identity_id,
        stream: Box::pin(stream),
        log: Some(log),
        log_id,
        started_at,
        stats,
        base_metadata_json,
        upstream_request_id,
        inserted: false,
        failed: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        let Some(item) = state.stream.next().await else {
            state.record_stream_end().await;
            return None;
        };

        if !state.inserted {
            state.insert_first_chunk_log(&item).await;
        } else if let Err(error) = &item {
            state.record_stream_error(error.to_string()).await;
        }

        Some((item, state))
    })
}

struct StreamRecordState {
    db: SqlitePool,
    identity_id: String,
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    log: Option<RequestLogInsert>,
    log_id: String,
    started_at: Instant,
    stats: Option<SharedStreamStats>,
    base_metadata_json: Option<String>,
    upstream_request_id: Option<String>,
    inserted: bool,
    failed: bool,
}

impl StreamRecordState {
    async fn insert_first_chunk_log(&mut self, item: &Result<Bytes, std::io::Error>) {
        let Some(mut log) = self.log.take() else {
            return;
        };
        let first_token_ms = self.started_at.elapsed().as_millis() as i64;
        log.latency_ms = first_token_ms;
        match item {
            Ok(_) => {
                log.first_token_ms = Some(first_token_ms);
                log.metadata_json = self.metadata(StreamMetadataUpdate {
                    empty: Some(false),
                    completed: None,
                    final_usage_seen: None,
                    stream_error: None,
                    upstream_request_id: self.upstream_request_id.clone(),
                    upstream_error_body_excerpt: None,
                });
            }
            Err(error) => {
                self.failed = true;
                log.status = "failed".to_string();
                log.http_status = 502;
                log.error_code = Some("stream_error".to_string());
                log.error_message = Some(error.to_string());
                log.first_token_ms = None;
                log.metadata_json = self.metadata(StreamMetadataUpdate {
                    empty: Some(false),
                    completed: Some(false),
                    final_usage_seen: Some(false),
                    stream_error: Some(error.to_string()),
                    upstream_request_id: self.upstream_request_id.clone(),
                    upstream_error_body_excerpt: None,
                });
            }
        }
        self.inserted = insert_stream_log_with_id(&self.db, &self.identity_id, &self.log_id, log)
            .await
            .is_ok();
    }

    async fn record_stream_end(&mut self) {
        let latency_ms = self.started_at.elapsed().as_millis() as i64;
        if !self.inserted {
            self.record_empty_stream(latency_ms).await;
            return;
        }
        if self.failed {
            return;
        }

        let snapshot = self.stats_snapshot();
        let completed = snapshot.completed || self.stats.is_none();
        let metadata_json = self.metadata(StreamMetadataUpdate {
            empty: Some(false),
            completed: Some(completed),
            final_usage_seen: Some(snapshot.final_usage_seen),
            stream_error: None,
            upstream_request_id: self.upstream_request_id.clone(),
            upstream_error_body_excerpt: None,
        });
        let update = StreamRequestLogUpdate {
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: None,
            latency_ms,
            tool_call_count: Some(snapshot.tool_call_count),
            upstream_request_id: self.upstream_request_id.clone(),
            metadata_json,
        };
        update_stream_log(&self.db, &self.identity_id, &self.log_id, update).await;
    }

    async fn record_empty_stream(&mut self, latency_ms: i64) {
        let Some(mut log) = self.log.take() else {
            return;
        };
        log.status = "failed".to_string();
        log.http_status = 502;
        log.latency_ms = latency_ms;
        log.error_code = Some("empty_stream".to_string());
        log.error_message = Some("upstream stream finished without chunks".to_string());
        log.metadata_json = self.metadata(StreamMetadataUpdate {
            empty: Some(true),
            completed: Some(false),
            final_usage_seen: Some(false),
            stream_error: None,
            upstream_request_id: self.upstream_request_id.clone(),
            upstream_error_body_excerpt: None,
        });
        if insert_stream_log_with_id(&self.db, &self.identity_id, &self.log_id, log)
            .await
            .is_ok()
        {
            self.inserted = true;
            self.failed = true;
        }
    }

    async fn record_stream_error(&mut self, message: String) {
        self.failed = true;
        let latency_ms = self.started_at.elapsed().as_millis() as i64;
        let snapshot = self.stats_snapshot();
        let metadata_json = self.metadata(StreamMetadataUpdate {
            empty: Some(false),
            completed: Some(false),
            final_usage_seen: Some(snapshot.final_usage_seen),
            stream_error: Some(message.clone()),
            upstream_request_id: self.upstream_request_id.clone(),
            upstream_error_body_excerpt: None,
        });
        let update = StreamRequestLogUpdate {
            status: "failed".to_string(),
            http_status: 502,
            error_code: Some("stream_error".to_string()),
            error_message: Some(message),
            input_tokens: snapshot.input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: None,
            latency_ms,
            tool_call_count: Some(snapshot.tool_call_count),
            upstream_request_id: self.upstream_request_id.clone(),
            metadata_json,
        };
        update_stream_log(&self.db, &self.identity_id, &self.log_id, update).await;
    }

    fn stats_snapshot(&self) -> StreamStatsSnapshot {
        self.stats
            .as_ref()
            .and_then(|stats| stats.lock().ok().map(|stats| stats.clone()))
            .unwrap_or_default()
    }

    fn metadata(&self, update: StreamMetadataUpdate) -> Option<String> {
        update_stream_metadata(self.base_metadata_json.as_deref(), &update).ok()
    }
}

async fn insert_stream_log_with_id(
    db: &SqlitePool,
    identity_id: &str,
    id: &str,
    log: RequestLogInsert,
) -> anyhow::Result<()> {
    if let Err(error) = insert_with_id(db, identity_id, id.to_string(), log).await {
        log_stream_storage_failure("insert", &error);
        return Err(error);
    }
    Ok(())
}

async fn update_stream_log(
    db: &SqlitePool,
    identity_id: &str,
    id: &str,
    update: StreamRequestLogUpdate,
) {
    if let Err(error) = update_stream(db, identity_id, id, update).await {
        log_stream_storage_failure("update", &error);
    }
}

fn log_stream_storage_failure(operation: &'static str, _error: &anyhow::Error) {
    tracing::error!(
        operation,
        failure_kind = "stream_log_storage",
        "failed to persist streaming request log"
    );
}

#[cfg(test)]
mod tests {
    use std::{
        fmt::{self, Write as _},
        sync::{Arc, Mutex},
    };

    use bytes::Bytes;
    use futures::{stream, TryStreamExt};
    use sqlx::sqlite::SqlitePoolOptions;
    use tracing::{
        field::{Field, Visit},
        Event, Subscriber,
    };
    use tracing_subscriber::{layer::Context, prelude::*, Layer, Registry};

    use super::{record_first_chunk, record_stream};
    use crate::{
        bridge::stream::StreamStatsSnapshot,
        observability::request_metadata::build_request_metadata,
        stats::RequestLogInsert,
        storage::{stats::insert_with_id, MasterKey, Storage},
    };

    #[tokio::test]
    async fn record_first_chunk_updates_completed_metadata_on_eof() {
        let (db, identity_id) = test_db().await;
        let metadata_json = test_metadata();
        let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))]);

        let chunks = record_first_chunk(
            db.clone(),
            identity_id,
            stream,
            test_log(metadata_json),
            std::time::Instant::now(),
        )
        .try_collect::<Vec<_>>()
        .await
        .expect("collect stream");

        assert_eq!(chunks, vec![Bytes::from_static(b"hello")]);

        let row: (i64, Option<String>) =
            sqlx::query_as("SELECT COUNT(*), metadata_json FROM identity_request_logs")
                .fetch_one(&db)
                .await
                .expect("load stream log");
        let metadata: serde_json::Value =
            serde_json::from_str(row.1.as_deref().expect("metadata")).expect("parse metadata");

        assert_eq!(row.0, 1);
        assert_eq!(metadata["stream"]["empty"], false);
        assert_eq!(metadata["stream"]["completed"], true);
        assert_eq!(metadata["stream"]["final_usage_seen"], false);
    }

    #[tokio::test]
    async fn record_stream_updates_usage_and_tool_count_on_eof() {
        let (db, identity_id) = test_db().await;
        let metadata_json = test_metadata();
        let stats = Arc::new(Mutex::new(StreamStatsSnapshot {
            input_tokens: Some(11),
            output_tokens: Some(7),
            total_tokens: Some(18),
            tool_call_count: 2,
            completed: true,
            final_usage_seen: true,
        }));
        let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))]);

        record_stream(
            db.clone(),
            identity_id,
            stream,
            test_log(metadata_json),
            std::time::Instant::now(),
            stats,
        )
        .try_collect::<Vec<_>>()
        .await
        .expect("collect stream");

        let row: (i64, Option<i64>, Option<i64>, Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT COUNT(*), input_tokens, output_tokens, tool_call_count, metadata_json FROM identity_request_logs",
        )
        .fetch_one(&db)
        .await
        .expect("load stream log");
        let metadata: serde_json::Value =
            serde_json::from_str(row.4.as_deref().expect("metadata")).expect("parse metadata");

        assert_eq!(row.0, 1);
        assert_eq!(row.1, Some(11));
        assert_eq!(row.2, Some(7));
        assert_eq!(row.3, Some(2));
        assert_eq!(metadata["stream"]["completed"], true);
        assert_eq!(metadata["stream"]["final_usage_seen"], true);
    }

    #[tokio::test]
    async fn record_stream_does_not_update_existing_row_when_first_insert_fails() {
        let (db, identity_id) = test_db().await;
        insert_with_id(
            &db,
            &identity_id,
            "duplicate-stream-log".to_string(),
            test_log(test_metadata()),
        )
        .await
        .expect("insert existing log");
        let stream = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"hello"))]);

        super::record_stream_with_log_id(
            db.clone(),
            identity_id,
            stream,
            test_log(test_metadata()),
            std::time::Instant::now(),
            None,
            "duplicate-stream-log".to_string(),
        )
        .try_collect::<Vec<_>>()
        .await
        .expect("collect stream");

        let row: (i64, Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT COUNT(*), tool_call_count, metadata_json FROM identity_request_logs WHERE id = 'duplicate-stream-log'",
        )
        .fetch_one(&db)
        .await
        .expect("load duplicate log");
        let metadata: serde_json::Value =
            serde_json::from_str(row.2.as_deref().expect("metadata")).expect("parse metadata");

        assert_eq!(row.0, 1);
        assert_eq!(row.1, None);
        assert_eq!(metadata["stream"]["completed"], serde_json::Value::Null);
    }

    async fn test_db() -> (sqlx::SqlitePool, String) {
        let storage = Storage::initialize(
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("create sqlite pool"),
            MasterKey::from_bytes([0; 32]),
        )
        .await
        .expect("initialize storage");
        let identity = storage
            .register_identity(
                "machine-1",
                "S-1-5-21-1",
                &crate::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity");
        (storage.pool().clone(), identity.identity_id)
    }

    fn test_metadata() -> String {
        build_request_metadata(
            "responses",
            "responses",
            crate::providers::spec::UpstreamProtocol::ChatCompletions,
            "coder",
            "deepseek-chat",
            Vec::new(),
        )
        .expect("build metadata")
    }

    fn test_log(metadata_json: String) -> RequestLogInsert {
        RequestLogInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "chat_completions".to_string(),
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
        let error = anyhow::anyhow!(
            "SQLite error: SELECT provider_key WHERE device_credential = 'device-secret'"
        );

        tracing::subscriber::with_default(subscriber, || {
            super::log_stream_storage_failure("insert", &error);
        });

        let events = events.lock().expect("lock captured tracing events");
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("operation=insert"));
        assert!(events[0].contains("failure_kind=stream_log_storage"));
        assert!(!events[0].contains("provider_key"));
        assert!(!events[0].contains("device_credential"));
        assert!(!events[0].contains("device-secret"));
    }
}
