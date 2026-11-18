use sea_orm::{DbBackend, MockDatabase};
use serde_json::json;
use tempfile::tempdir;

use super::{
    pending_response_state, RecordingHealthStatus, RecordingRuntime, RecordingRuntimeConfig,
};
use crate::{
    codec::internal::InternalResponse,
    domain::{activity::SessionContext, stats::ActivityInsert},
    store::{
        recording_journal::{
            GroupCommitConfig, RecordEnvelope, RecordKind, ResponseStateRecord,
            TurnCompletedRecord, TurnTiming,
        },
        rollout::RolloutStore,
        storage::{MasterKey, Storage},
    },
};

#[test]
fn builds_pending_response_state_from_the_committed_envelope() {
    let payload = TurnCompletedRecord {
        activity: ActivityInsert::default(),
        session: SessionContext::new("thread-pending"),
        request_events: Vec::new(),
        response_events: Vec::new(),
        response_state: Some(ResponseStateRecord {
            response_id: "resp_pending".to_string(),
            previous_response_id: None,
            conversation_id: "conversation-pending".to_string(),
            provider_id: "provider-pending".to_string(),
            model: "model-pending".to_string(),
            upstream_response_id: None,
            request_items: json!([{"role": "user", "content": "hello"}]),
            response_items: json!([{"type": "message"}]),
            input_messages: Vec::new(),
            response: InternalResponse {
                id: "resp_pending".to_string(),
                model: "model-pending".to_string(),
                output: Vec::new(),
                usage: None,
            },
        }),
        timing: TurnTiming {
            started_at: "2026-11-17T12:00:00+08:00".to_string(),
            first_token_at: None,
            finished_at: Some("2026-11-17T12:00:01+08:00".to_string()),
        },
    };
    let envelope = RecordEnvelope::from_payload(
        "activity-pending",
        "identity-pending",
        RecordKind::TurnCompleted,
        "2026-11-17T12:00:00+08:00",
        &payload,
    )
    .expect("build envelope");

    let state = pending_response_state(&envelope).expect("pending response state");
    assert_eq!(state.identity_id, "identity-pending");
    assert_eq!(state.response_id, "resp_pending");
    assert_eq!(state.created_at, "2026-11-17T04:00:01+00:00");
    assert_eq!(
        state.request_items,
        json!([{"role": "user", "content": "hello"}])
    );
}

#[tokio::test]
async fn starts_in_a_fresh_journal_directory_and_shuts_down_cleanly() {
    let directory = tempdir().expect("temporary directory");
    let journal_dir = directory.path().join("recording");
    let storage = Storage::from_connection(
        MockDatabase::new(DbBackend::Postgres).into_connection(),
        MasterKey::from_bytes([0; 32]),
    );
    let rollouts = RolloutStore::new(directory.path().join("sessions"));
    let runtime = RecordingRuntime::start(
        RecordingRuntimeConfig {
            journal_dir: journal_dir.clone(),
            segment_bytes: 4096,
            chunk_bytes: 1024,
            group_commit: GroupCommitConfig::default(),
            materialize_interval: std::time::Duration::from_millis(10),
            commit_timeout: std::time::Duration::from_secs(1),
            min_free_bytes: 0,
            max_pending_bytes: 0,
            materialize_lag_slo_seconds: 5,
        },
        &MasterKey::from_bytes([0; 32]),
        storage,
        rollouts,
    )
    .await
    .expect("start recording runtime");

    assert_eq!(
        runtime.health_snapshot().status,
        RecordingHealthStatus::Ready
    );
    assert!(journal_dir.join("node-id").is_file());
    runtime.shutdown().await;
}

#[tokio::test]
async fn commits_an_envelope_and_exposes_response_state_before_materialization() {
    let directory = tempdir().expect("temporary directory");
    let journal_dir = directory.path().join("recording");
    let storage = Storage::from_connection(
        MockDatabase::new(DbBackend::Postgres).into_connection(),
        MasterKey::from_bytes([0; 32]),
    );
    let rollouts = RolloutStore::new(directory.path().join("sessions"));
    let runtime = RecordingRuntime::start(
        RecordingRuntimeConfig {
            journal_dir,
            segment_bytes: 4096,
            chunk_bytes: 1024,
            group_commit: GroupCommitConfig::default(),
            materialize_interval: std::time::Duration::from_millis(10),
            commit_timeout: std::time::Duration::from_secs(1),
            min_free_bytes: 0,
            max_pending_bytes: 0,
            materialize_lag_slo_seconds: 5,
        },
        &MasterKey::from_bytes([0; 32]),
        storage,
        rollouts,
    )
    .await
    .expect("start recording runtime");
    let finished_at = "2026-11-17T12:00:01+08:00";
    let envelope = RecordEnvelope::from_payload(
        "activity-commit",
        "identity-commit",
        RecordKind::TurnCompleted,
        finished_at,
        &TurnCompletedRecord {
            activity: ActivityInsert::default(),
            session: SessionContext::new("thread-commit"),
            request_events: Vec::new(),
            response_events: Vec::new(),
            response_state: Some(ResponseStateRecord {
                response_id: "resp_commit".to_string(),
                previous_response_id: None,
                conversation_id: "conversation-commit".to_string(),
                provider_id: "provider-commit".to_string(),
                model: "model-commit".to_string(),
                upstream_response_id: None,
                request_items: json!([]),
                response_items: json!([]),
                input_messages: Vec::new(),
                response: InternalResponse {
                    id: "resp_commit".to_string(),
                    model: "model-commit".to_string(),
                    output: Vec::new(),
                    usage: None,
                },
            }),
            timing: TurnTiming {
                started_at: "2026-11-17T12:00:00+08:00".to_string(),
                first_token_at: None,
                finished_at: Some(finished_at.to_string()),
            },
        },
    )
    .expect("build envelope");

    runtime
        .commit_envelope(envelope)
        .await
        .expect("commit envelope");
    let state = runtime
        .pending_response_state("identity-commit", "resp_commit")
        .expect("pending response state");
    assert_eq!(state.response_id, "resp_commit");
    assert!(runtime.health_snapshot().durable_sequence > 0);
    runtime.shutdown().await;
}

#[tokio::test]
async fn rejects_a_second_runtime_on_the_same_journal_directory() {
    let directory = tempdir().expect("temporary directory");
    let journal_dir = directory.path().join("recording");
    let storage = || {
        Storage::from_connection(
            MockDatabase::new(DbBackend::Postgres).into_connection(),
            MasterKey::from_bytes([0; 32]),
        )
    };
    let first = RecordingRuntime::start(
        RecordingRuntimeConfig {
            journal_dir: journal_dir.clone(),
            segment_bytes: 4096,
            chunk_bytes: 1024,
            group_commit: GroupCommitConfig::default(),
            materialize_interval: std::time::Duration::from_millis(10),
            commit_timeout: std::time::Duration::from_secs(1),
            min_free_bytes: 0,
            max_pending_bytes: 0,
            materialize_lag_slo_seconds: 5,
        },
        &MasterKey::from_bytes([0; 32]),
        storage(),
        RolloutStore::new(directory.path().join("sessions-first")),
    )
    .await
    .expect("start first runtime");
    let error = match RecordingRuntime::start(
        RecordingRuntimeConfig {
            journal_dir,
            segment_bytes: 4096,
            chunk_bytes: 1024,
            group_commit: GroupCommitConfig::default(),
            materialize_interval: std::time::Duration::from_millis(10),
            commit_timeout: std::time::Duration::from_secs(1),
            min_free_bytes: 0,
            max_pending_bytes: 0,
            materialize_lag_slo_seconds: 5,
        },
        &MasterKey::from_bytes([0; 32]),
        storage(),
        RolloutStore::new(directory.path().join("sessions-second")),
    )
    .await
    {
        Ok(_) => panic!("second runtime must not share the journal"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("locked") || error.to_string().contains("lock"));
    first.shutdown().await;
}
