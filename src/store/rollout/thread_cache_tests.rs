use serde_json::json;

use super::{RolloutStore, ThreadSource};
use crate::domain::activity::rollout::RolloutStatus;
use crate::domain::activity::{
    EventPhase, MessageEvent, RolloutEventBody, SessionContext, SessionMetaEvent,
};

fn test_meta(activity_id: &str, session_id: &str) -> SessionMetaEvent {
    SessionMetaEvent {
        activity_id: activity_id.to_string(),
        session_id: session_id.to_string(),
        identity_id: "identity-1".to_string(),
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: Some("responses".to_string()),
        endpoint_name: "Codex".to_string(),
        model_requested: "deepseek-flash".to_string(),
        model_upstream: "deepseek/deepseek-v4.1-flash".to_string(),
        started_at: "2026-10-22T20:20:10Z".to_string(),
    }
}

fn message(phase: EventPhase, content: serde_json::Value) -> RolloutEventBody {
    RolloutEventBody::Message(MessageEvent {
        phase,
        role: "user".to_string(),
        content,
    })
}

fn session(thread_id: &str) -> SessionContext {
    SessionContext::new(thread_id)
}

#[tokio::test]
async fn reuses_cached_thread_state_after_the_first_load() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-cache");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-cache", "thread-cache"),
            vec![message(EventPhase::Request, json!("first"))],
        )
        .await
        .expect("begin cached thread");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish cached thread");

    let fresh = RolloutStore::new(root.path().to_path_buf());
    let loaded = fresh
        .resolve_thread_source("identity-1", "thread-cache")
        .expect("load thread");
    assert!(matches!(loaded, ThreadSource::Loaded(_, _)));

    let cached = fresh
        .resolve_thread_source("identity-1", "thread-cache")
        .expect("load cached thread");
    assert!(matches!(cached, ThreadSource::Cached(_)));
}

#[tokio::test]
async fn forgetting_a_thread_drops_its_cached_state() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-forget");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-forget", "thread-forget"),
            vec![message(EventPhase::Request, json!("first"))],
        )
        .await
        .expect("begin forgotten thread");
    writer
        .finish(RolloutStatus::Completed)
        .expect("finish forgotten thread");

    store.forget_relative_paths(&["identity-1/thread-forget.jsonl".to_string()]);

    let loaded = store
        .resolve_thread_source("identity-1", "thread-forget")
        .expect("reload forgotten thread");
    assert!(matches!(loaded, ThreadSource::Loaded(_, _)));
}

#[tokio::test]
async fn falls_back_to_a_full_read_when_the_cached_history_does_not_match() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-diverged");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-first", "thread-diverged"),
            vec![message(EventPhase::Request, json!("first user"))],
        )
        .await
        .expect("begin first activity");
    first
        .append(message(EventPhase::Response, json!("first answer")))
        .expect("append first answer");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first activity");

    let mut second = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-second", "thread-diverged"),
            vec![message(EventPhase::Request, json!("different user"))],
        )
        .await
        .expect("begin divergent activity");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish divergent activity");

    let events = store
        .read_events("identity-1/thread-diverged.jsonl")
        .expect("read divergent thread");
    let contents = events
        .iter()
        .filter(|event| event.event_type == "message")
        .map(|event| event.payload["content"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        contents,
        vec![
            json!("first user"),
            json!("first answer"),
            json!("different user")
        ]
    );
}
