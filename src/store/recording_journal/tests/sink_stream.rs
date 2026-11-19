use std::collections::BTreeMap;

use sea_orm::{DbBackend, MockDatabase, Value};
use tempfile::tempdir;

use crate::{
    domain::{
        activity::SessionContext,
        stats::{ActivityInsert, StreamActivityUpdate},
    },
    store::{
        recording_journal::{
            materializer::MaterializeSink,
            payload::{RecordedContentEvent, TurnFinishedRecord, TurnStartedRecord, TurnTiming},
            record::{RecordEnvelope, RecordKind},
            sink::ActivityMaterializeSink,
            PendingResponseStates,
        },
        rollout::RolloutStore,
        storage::{MasterKey, Storage},
    },
};

use super::sink::{activity_model, usage_event};

#[tokio::test]
async fn rewrites_the_activity_row_when_a_started_turn_was_already_finished() {
    let directory = tempdir().expect("temporary rollout directory");
    let rollouts = RolloutStore::new(directory.path().to_path_buf());
    let session = SessionContext::new("thread-replayed-start");
    // 上一次物化写完 turn_end 之后、写数据库之前中断：rollout 已完成，活动行缺失。
    let mut writer = rollouts
        .begin(
            "identity-1",
            &session,
            crate::domain::activity::TurnContext {
                activity_id: "activity-replayed-start".to_string(),
                model_requested: "model-1".to_string(),
                model_upstream: "model-upstream".to_string(),
            },
            vec![usage_event(10, 0)],
        )
        .await
        .expect("begin turn");
    writer
        .finish(crate::domain::activity::RolloutStatus::Completed)
        .expect("finish turn");
    drop(writer);

    let string = |value: &str| Value::String(Some(value.to_string()));
    let database = MockDatabase::new(DbBackend::Postgres)
        .append_query_results([
            vec![BTreeMap::from([(
                "id".to_string(),
                string("activity-replayed-start"),
            )])],
            vec![BTreeMap::from([(
                "activity_id".to_string(),
                string("activity-replayed-start"),
            )])],
        ])
        .into_connection();
    let storage = Storage::from_connection(database.clone(), MasterKey::from_bytes([0; 32]));
    let sink = ActivityMaterializeSink::new(storage, rollouts, PendingResponseStates::default());
    let envelope = RecordEnvelope::from_payload(
        "activity-replayed-start",
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
            session,
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

    sink.apply(envelope)
        .await
        .expect("a replayed start must still persist the activity row");
    let statements = database
        .into_transaction_log()
        .iter()
        .flat_map(|transaction| transaction.statements())
        .map(|statement| statement.sql.clone())
        .collect::<Vec<_>>();
    assert!(
        statements.iter().any(|statement| {
            statement.contains("identity_activities") && statement.contains("INSERT")
        }),
        "an already finished rollout turn must not skip the activity row"
    );
}

#[tokio::test]
async fn completes_a_finished_turn_whose_activity_row_was_never_written() {
    let directory = tempdir().expect("temporary rollout directory");
    let rollouts = RolloutStore::new(directory.path().to_path_buf());
    let session = SessionContext::new("thread-missing-row");
    let writer = rollouts
        .begin(
            "identity-1",
            &session,
            crate::domain::activity::TurnContext {
                activity_id: "activity-missing-row".to_string(),
                model_requested: "model-1".to_string(),
                model_upstream: "model-upstream".to_string(),
            },
            vec![usage_event(10, 0)],
        )
        .await
        .expect("begin turn");
    drop(writer);

    let string = |value: &str| Value::String(Some(value.to_string()));
    let database = MockDatabase::new(DbBackend::Postgres)
        // 活动行不存在：这正是 checkpoint 越过 turn_started 之后的现场。
        .append_query_results([Vec::<BTreeMap<String, Value>>::new()])
        // 用 turn_finished 自带的快照插入活动行。
        .append_query_results([vec![activity_model()]])
        .append_query_results([vec![BTreeMap::from([(
            "activity_id".to_string(),
            string("activity-missing-row"),
        )])]])
        .into_connection();
    let storage = Storage::from_connection(database.clone(), MasterKey::from_bytes([0; 32]));
    let sink = ActivityMaterializeSink::new(storage, rollouts, PendingResponseStates::default());
    let envelope = RecordEnvelope::from_payload(
        "activity-missing-row",
        "identity-1",
        RecordKind::TurnFinished,
        "2026-11-17T12:00:01.000+08:00",
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
            activity_snapshot: Some(ActivityInsert {
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
            }),
            timing: TurnTiming {
                started_at: "2026-11-17T12:00:00.000+08:00".to_string(),
                first_token_at: Some("2026-11-17T12:00:00.100+08:00".to_string()),
                finished_at: Some("2026-11-17T12:00:01.000+08:00".to_string()),
            },
        },
    )
    .expect("build finish envelope");

    sink.apply(envelope)
        .await
        .expect("a finish record must be able to recreate a missing activity row");
    let statements = database
        .into_transaction_log()
        .iter()
        .flat_map(|transaction| transaction.statements())
        .map(|statement| statement.sql.clone())
        .collect::<Vec<_>>();
    assert!(
        statements.iter().any(|statement| {
            statement.contains("identity_activities") && statement.contains("INSERT")
        }),
        "the missing activity row must be inserted from the finish snapshot"
    );
}
