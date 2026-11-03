use serde_json::json;

use super::RolloutStore;
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
        started_at: "2026-11-03T00:00:00Z".to_string(),
    }
}

fn message(role: &str, content: &str) -> RolloutEventBody {
    RolloutEventBody::Message(MessageEvent {
        phase: EventPhase::Request,
        role: role.to_string(),
        content: json!(content),
    })
}

fn session(thread_id: &str) -> SessionContext {
    SessionContext::new(thread_id)
}

fn message_contents(store: &RolloutStore, path: &str) -> Vec<(String, String)> {
    store
        .read_events(path)
        .expect("read thread events")
        .into_iter()
        .filter(|event| event.event_type == "message")
        .map(|event| {
            (
                event.payload["role"].as_str().unwrap_or("").to_string(),
                event.payload["content"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn reordered_history_only_appends_new_events() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-reordered");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-first", "thread-reordered"),
            vec![message("user", "first"), message("user", "second")],
        )
        .await
        .expect("begin first activity");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first activity");

    let mut second = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-second", "thread-reordered"),
            vec![
                message("user", "third"),
                message("user", "first"),
                message("user", "second"),
            ],
        )
        .await
        .expect("begin reordered history");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish reordered history");

    assert_eq!(
        message_contents(&store, "identity-1/thread-reordered.jsonl"),
        vec![
            ("user".to_string(), "first".to_string()),
            ("user".to_string(), "second".to_string()),
            ("user".to_string(), "third".to_string())
        ]
    );
}

#[tokio::test]
async fn repeated_identical_messages_append_only_new_occurrences() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-repeat");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-first", "thread-repeat"),
            vec![message("user", "repeat")],
        )
        .await
        .expect("begin first activity");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first activity");

    let mut second = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-second", "thread-repeat"),
            vec![message("user", "repeat"), message("user", "repeat")],
        )
        .await
        .expect("begin repeated history");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish repeated history");

    assert_eq!(
        message_contents(&store, "identity-1/thread-repeat.jsonl"),
        vec![
            ("user".to_string(), "repeat".to_string()),
            ("user".to_string(), "repeat".to_string())
        ]
    );
}

#[tokio::test]
async fn repeated_delta_input_is_appended_when_history_is_not_replayed() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-delta");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-first", "thread-delta"),
            vec![message("user", "repeat")],
        )
        .await
        .expect("begin first activity");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first activity");

    let delta_session = session.with_replay_history(false);
    let mut second = store
        .begin(
            "identity-1",
            &delta_session,
            test_meta("activity-second", "thread-delta"),
            vec![message("user", "repeat")],
        )
        .await
        .expect("begin repeated delta input");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish repeated delta input");

    assert_eq!(
        message_contents(&store, "identity-1/thread-delta.jsonl"),
        vec![
            ("user".to_string(), "repeat".to_string()),
            ("user".to_string(), "repeat".to_string())
        ]
    );
}

#[tokio::test]
async fn omitted_initial_system_message_does_not_replay_user_history() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-system");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-first", "thread-system"),
            vec![message("system", "rules"), message("user", "hello")],
        )
        .await
        .expect("begin first activity");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first activity");

    let mut second = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-second", "thread-system"),
            vec![message("user", "hello"), message("user", "next")],
        )
        .await
        .expect("begin history without system prefix");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish history without system prefix");

    assert_eq!(
        message_contents(&store, "identity-1/thread-system.jsonl"),
        vec![
            ("system".to_string(), "rules".to_string()),
            ("user".to_string(), "hello".to_string()),
            ("user".to_string(), "next".to_string())
        ]
    );
}
