use std::collections::BTreeMap;

use sea_orm::{DbBackend, MockDatabase, MockExecResult, Value};
use serde_json::json;
use tempfile::tempdir;

use crate::store::entity::identity::activities;
use crate::{
    codec::internal::{InternalResponse, InternalUsage},
    domain::{
        activity::{ContentDraft, ContentEventBody, ConversationUsage, SessionContext},
        stats::{ActivityInsert, StreamActivityUpdate},
    },
    store::{
        recording_journal::{
            materializer::{MaterializeError, MaterializeSink},
            payload::{
                ProviderObservationRecord, RecordedContentEvent, ResponseStateRecord,
                TurnCompletedRecord, TurnFinishedRecord, TurnStartedRecord, TurnTiming,
            },
            record::{RecordEnvelope, RecordKind},
            sink::ActivityMaterializeSink,
            PendingResponseStates,
        },
        rollout::{RolloutStore, StartedTurnMaterialization, TimedContentDraft},
        storage::{MasterKey, ResponseState, Storage},
    },
};

#[tokio::test]
async fn rejects_a_malformed_provider_observation() {
    let directory = tempdir().expect("temporary rollout directory");
    let database = MockDatabase::new(DbBackend::Postgres).into_connection();
    let storage = Storage::from_connection(database, MasterKey::from_bytes([0; 32]));
    let rollouts = RolloutStore::new(directory.path().to_path_buf());
    let sink = ActivityMaterializeSink::new(storage, rollouts, PendingResponseStates::default());
    let envelope = RecordEnvelope::from_payload(
        "activity-1",
        "identity-1",
        RecordKind::ProviderObservation,
        "2026-11-17T12:00:00+08:00",
        &json!({"unsupported": true}),
    )
    .expect("build envelope");

    let error = sink
        .apply(envelope)
        .await
        .expect_err("malformed provider observation must fail");
    assert!(matches!(error, MaterializeError::Journal(_)));
}

#[tokio::test]
async fn materializes_a_provider_observation_without_blocking_the_batch() {
    let directory = tempdir().expect("temporary rollout directory");
    let database = MockDatabase::new(DbBackend::Postgres)
        .append_exec_results([MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        }])
        .into_connection();
    let storage = Storage::from_connection(database.clone(), MasterKey::from_bytes([0; 32]));
    let rollouts = RolloutStore::new(directory.path().to_path_buf());
    let sink = ActivityMaterializeSink::new(storage, rollouts, PendingResponseStates::default());
    let observed_at = "2026-11-17T12:00:00+08:00";
    let envelope = RecordEnvelope::from_payload(
        "observation-1",
        "identity-1",
        RecordKind::ProviderObservation,
        observed_at,
        &ProviderObservationRecord {
            endpoint_id: "endpoint-1".to_string(),
            endpoint_name: "Endpoint One".to_string(),
            model_name: "model-1".to_string(),
            provider_id: "provider-1".to_string(),
            observed_at: observed_at.to_string(),
        },
    )
    .expect("build envelope");

    sink.apply(envelope)
        .await
        .expect("materialize provider observation");
    let statements = database
        .into_transaction_log()
        .iter()
        .flat_map(|transaction| transaction.statements())
        .map(|statement| statement.sql.clone())
        .collect::<Vec<_>>();
    assert!(statements
        .iter()
        .any(|statement| statement.contains("endpoint_model_routes")));
}

#[tokio::test]
async fn materializes_a_completed_turn_into_the_thread_and_database() {
    let directory = tempdir().expect("temporary rollout directory");
    let string = |value: &str| Value::String(Some(value.to_string()));
    let database = MockDatabase::new(DbBackend::Postgres)
        .append_query_results([
            vec![BTreeMap::from([(
                "id".to_string(),
                string("activity-sink"),
            )])],
            vec![BTreeMap::from([(
                "activity_id".to_string(),
                string("activity-sink"),
            )])],
            vec![BTreeMap::from([
                ("response_id".to_string(), string("resp-sink")),
                ("identity_id".to_string(), string("identity-1")),
            ])],
        ])
        .into_connection();
    let storage = Storage::from_connection(database.clone(), MasterKey::from_bytes([0; 32]));
    let rollouts = RolloutStore::new(directory.path().to_path_buf());
    let pending = PendingResponseStates::default();
    pending.insert(ResponseState {
        identity_id: "identity-1".to_string(),
        response_id: "resp-sink".to_string(),
        previous_response_id: None,
        conversation_id: Some("conversation-1".to_string()),
        provider_id: "provider-1".to_string(),
        model: "model-1".to_string(),
        upstream_response_id: Some("resp-upstream".to_string()),
        request_items: json!([]),
        response_items: json!([]),
        created_at: "2026-11-17T12:00:01+08:00".to_string(),
        expires_at: "2026-12-17T12:00:01+08:00".to_string(),
    });
    let sink = ActivityMaterializeSink::new(storage, rollouts.clone(), pending.clone());
    let envelope = RecordEnvelope::from_payload(
        "activity-sink",
        "identity-1",
        RecordKind::TurnCompleted,
        "2026-11-17T12:00:00.000+08:00",
        &TurnCompletedRecord {
            activity: ActivityInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: "provider-1".to_string(),
                provider_name: "Provider One".to_string(),
                provider_type: "openai".to_string(),
                model_requested: "model-1".to_string(),
                model_upstream: "model-upstream".to_string(),
                status: "success".to_string(),
                http_status: 200,
                input_tokens: Some(10),
                output_tokens: Some(4),
                latency_ms: 100,
                ..Default::default()
            },
            session: SessionContext::new("thread-sink"),
            request_events: vec![RecordedContentEvent::new(
                "2026-11-17T12:00:00.100+08:00",
                usage_event(10, 0),
            )],
            response_events: vec![RecordedContentEvent::new(
                "2026-11-17T12:00:00.900+08:00",
                usage_event(10, 4),
            )],
            response_state: Some(ResponseStateRecord {
                response_id: "resp-sink".to_string(),
                previous_response_id: None,
                conversation_id: "conversation-1".to_string(),
                provider_id: "provider-1".to_string(),
                model: "model-1".to_string(),
                upstream_response_id: Some("resp-upstream".to_string()),
                request_items: json!([{"role": "user", "content": "hello"}]),
                response_items: json!([{"type": "message"}]),
                input_messages: Vec::new(),
                response: InternalResponse {
                    id: "resp-sink".to_string(),
                    model: "model-1".to_string(),
                    output: Vec::new(),
                    usage: Some(InternalUsage {
                        input_tokens: Some(10),
                        output_tokens: Some(4),
                        reasoning_tokens: None,
                        cache_read_tokens: None,
                        cache_write_tokens: None,
                    }),
                },
            }),
            timing: TurnTiming {
                started_at: "2026-11-17T12:00:00.000+08:00".to_string(),
                first_token_at: None,
                finished_at: Some("2026-11-17T12:00:01.000+08:00".to_string()),
            },
        },
    )
    .expect("build envelope");

    sink.apply(envelope).await.expect("materialize turn");
    assert!(pending.get("identity-1", "resp-sink").is_none());
    let file = rollouts
        .read_verified("identity-1/thread-sink.jsonl")
        .expect("read materialized thread");
    assert_eq!(file.events.last().expect("turn end").event_type, "turn_end");
    assert_eq!(
        file.events
            .iter()
            .find(|event| event.event_type == "turn_end")
            .expect("turn end")
            .time,
        "2026-11-17T12:00:01.000+08:00"
    );
    let transactions = database.into_transaction_log();
    let statements = transactions
        .iter()
        .flat_map(|transaction| transaction.statements())
        .collect::<Vec<_>>();
    assert!(statements
        .iter()
        .any(|statement| statement.sql.contains("identity_activities")));
    assert!(statements
        .iter()
        .any(|statement| statement.sql.contains("activity_rollouts")));
    assert!(statements
        .iter()
        .any(|statement| statement.sql.contains("identity_response_sessions")));
    let stored_times = statements
        .iter()
        .flat_map(|statement| statement.values.as_ref().into_iter())
        .flat_map(|values| values.0.iter())
        .filter_map(|value| match value {
            Value::String(Some(value)) => Some(value.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(stored_times.contains(&"2026-11-17T12:00:00.000+08:00"));
    assert!(stored_times.contains(&"2026-11-17T12:00:01.000+08:00"));
}

#[tokio::test]
async fn materializes_a_started_stream_turn_as_a_writing_placeholder() {
    let directory = tempdir().expect("temporary rollout directory");
    let string = |value: &str| Value::String(Some(value.to_string()));
    let database = MockDatabase::new(DbBackend::Postgres)
        .append_query_results([
            vec![BTreeMap::from([(
                "id".to_string(),
                string("activity-stream-sink"),
            )])],
            vec![BTreeMap::from([(
                "activity_id".to_string(),
                string("activity-stream-sink"),
            )])],
        ])
        .into_connection();
    let storage = Storage::from_connection(database.clone(), MasterKey::from_bytes([0; 32]));
    let rollouts = RolloutStore::new(directory.path().to_path_buf());
    let sink =
        ActivityMaterializeSink::new(storage, rollouts.clone(), PendingResponseStates::default());
    let envelope = RecordEnvelope::from_payload(
        "activity-stream-sink",
        "identity-1",
        RecordKind::TurnStarted,
        "2026-11-17T12:00:00.000+08:00",
        &TurnStartedRecord {
            activity: ActivityInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: "provider-1".to_string(),
                provider_name: "Provider One".to_string(),
                provider_type: "openai".to_string(),
                model_requested: "model-1".to_string(),
                model_upstream: "model-upstream".to_string(),
                status: "streaming".to_string(),
                http_status: 200,
                is_streaming: true,
                ..Default::default()
            },
            session: SessionContext::new("thread-stream-sink"),
            request_events: vec![RecordedContentEvent::new(
                "2026-11-17T12:00:00.100+08:00",
                usage_event(10, 0),
            )],
            timing: TurnTiming {
                started_at: "2026-11-17T12:00:00.000+08:00".to_string(),
                first_token_at: None,
                finished_at: None,
            },
        },
    )
    .expect("build start envelope");

    sink.apply(envelope).await.expect("materialize start");
    let events = rollouts
        .read_events("identity-1/thread-stream-sink.jsonl")
        .expect("read started thread");
    assert!(events.iter().any(|event| event.event_type == "turn"));
    assert!(!events.iter().any(|event| event.event_type == "turn_end"));
    let transactions = database.into_transaction_log();
    let statements = transactions
        .iter()
        .flat_map(|transaction| transaction.statements())
        .collect::<Vec<_>>();
    assert!(statements
        .iter()
        .any(|statement| statement.sql.contains("identity_activities")));
    assert!(statements
        .iter()
        .any(|statement| statement.sql.contains("activity_rollouts")));
}

#[tokio::test]
async fn materializes_a_finished_stream_turn_and_updates_the_activity() {
    let directory = tempdir().expect("temporary rollout directory");
    let rollouts = RolloutStore::new(directory.path().to_path_buf());
    let session = SessionContext::new("thread-stream-sink");
    rollouts
        .materialize_started_turn(
            "identity-1",
            StartedTurnMaterialization {
                session: session.clone(),
                turn: crate::domain::activity::TurnContext {
                    activity_id: "activity-stream-sink".to_string(),
                    model_requested: "model-1".to_string(),
                    model_upstream: "model-upstream".to_string(),
                },
                created_at: "2026-11-17T12:00:00.000+08:00".to_string(),
                request_events: vec![TimedContentDraft::new(
                    "2026-11-17T12:00:00.100+08:00",
                    usage_event(10, 0),
                )],
            },
        )
        .await
        .expect("prepare started turn");
    let string = |value: &str| Value::String(Some(value.to_string()));
    let database = MockDatabase::new(DbBackend::Postgres)
        .append_query_results([vec![activity_model()], vec![activity_model()]])
        .append_query_results([vec![BTreeMap::from([(
            "activity_id".to_string(),
            string("activity-stream-sink"),
        )])]])
        .into_connection();
    let storage = Storage::from_connection(database.clone(), MasterKey::from_bytes([0; 32]));
    let sink =
        ActivityMaterializeSink::new(storage, rollouts.clone(), PendingResponseStates::default());
    let envelope = RecordEnvelope::from_payload(
        "activity-stream-sink",
        "identity-1",
        RecordKind::TurnFinished,
        "2026-11-17T12:00:00.000+08:00",
        &TurnFinishedRecord {
            activity: StreamActivityUpdate {
                status: "success".to_string(),
                http_status: 200,
                input_tokens: Some(10),
                output_tokens: Some(4),
                latency_ms: 900,
                ..Default::default()
            },
            session,
            response_events: vec![RecordedContentEvent::new(
                "2026-11-17T12:00:00.900+08:00",
                usage_event(10, 4),
            )],
            response_state: None,
            activity_snapshot: None,
            timing: TurnTiming {
                started_at: "2026-11-17T12:00:00.000+08:00".to_string(),
                first_token_at: Some("2026-11-17T12:00:00.100+08:00".to_string()),
                finished_at: Some("2026-11-17T12:00:01.000+08:00".to_string()),
            },
        },
    )
    .expect("build finish envelope");

    sink.apply(envelope).await.expect("materialize finish");
    let events = rollouts
        .read_events("identity-1/thread-stream-sink.jsonl")
        .expect("read finished thread");
    assert!(events.iter().any(|event| {
        event.event_type == "usage" && event.payload["output_tokens"] == json!(4)
    }));
    assert!(events.iter().any(|event| event.event_type == "turn_end"));
    let transactions = database.into_transaction_log();
    let statements = transactions
        .iter()
        .flat_map(|transaction| transaction.statements())
        .collect::<Vec<_>>();
    assert!(statements
        .iter()
        .any(|statement| statement.sql.contains("UPDATE")
            && statement.sql.contains("identity_activities")));
    assert!(statements
        .iter()
        .any(|statement| statement.sql.contains("activity_rollouts")));
}

pub(super) fn activity_model() -> activities::Model {
    activities::Model {
        id: "activity-stream-sink".to_string(),
        identity_id: "identity-1".to_string(),
        created_at: "2026-11-17T12:00:00.000+08:00".to_string(),
        protocol_in: Some("responses".to_string()),
        protocol_out: Some("responses".to_string()),
        protocol_upstream: Some("chat_completions".to_string()),
        endpoint_name: None,
        provider_id: Some("provider-1".to_string()),
        provider_name: Some("Provider One".to_string()),
        provider_type: Some("openai".to_string()),
        model_requested: Some("model-1".to_string()),
        model_upstream: Some("model-upstream".to_string()),
        status: "streaming".to_string(),
        http_status: Some(200),
        error_code: None,
        error_message: None,
        is_streaming: Some(true),
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms: Some(0),
        upstream_latency_ms: None,
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: None,
        metadata_json: None,
    }
}

pub(super) fn usage_event(input_tokens: i64, output_tokens: i64) -> ContentDraft {
    ContentDraft::new(ContentEventBody::Usage(ConversationUsage {
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
    }))
}
