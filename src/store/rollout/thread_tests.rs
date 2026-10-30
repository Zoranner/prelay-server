use serde_json::json;
use std::io::Write;

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
async fn only_appends_the_new_tail_of_a_thread_history() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-history");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-first", "thread-history"),
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

    let history = vec![
        message(EventPhase::Request, json!("first user")),
        message(EventPhase::Request, json!("first answer")),
        message(EventPhase::Request, json!("second user")),
    ];
    let mut second = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-second", "thread-history"),
            history,
        )
        .await
        .expect("begin second activity");
    let summary = second
        .finish(RolloutStatus::Completed)
        .expect("finish second activity");

    let events = store
        .read_events("identity-1/thread-history.jsonl")
        .expect("read thread events");
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
            json!("second user")
        ]
    );
    assert_eq!(summary.event_count, 1);
    assert_eq!(summary.start_line, 5);
    assert_eq!(summary.end_line, 5);
}

#[tokio::test]
async fn generation_change_starts_a_new_window_without_prefix_matching() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let first_session = session("thread-compact");
    let mut first = store
        .begin(
            "identity-1",
            &first_session,
            test_meta("activity-before-compact", "thread-compact"),
            vec![message(EventPhase::Request, json!("before compact"))],
        )
        .await
        .expect("begin first window");
    first
        .append(message(EventPhase::Response, json!("old answer")))
        .expect("append old answer");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first window");

    let compacted =
        SessionContext::with_window("thread-compact", Some("thread-compact:1".to_string()), 1);
    let mut second = store
        .begin(
            "identity-1",
            &compacted,
            test_meta("activity-after-compact", "thread-compact"),
            vec![message(EventPhase::Request, json!("after compact"))],
        )
        .await
        .expect("begin compacted window");
    let summary = second
        .finish(RolloutStatus::Completed)
        .expect("finish compacted window");

    let events = store
        .read_events("identity-1/thread-compact.jsonl")
        .expect("read compacted thread");
    let windows = events
        .iter()
        .filter(|event| event.event_type == "window_start")
        .map(|event| event.payload["generation"].clone())
        .collect::<Vec<_>>();
    assert_eq!(windows, vec![json!(0), json!(1)]);
    assert!(events.iter().any(|event| {
        event.event_type == "message" && event.payload["content"] == json!("before compact")
    }));
    assert!(events.iter().any(|event| {
        event.event_type == "message" && event.payload["content"] == json!("after compact")
    }));
    assert_eq!(summary.generation, 1);
    assert_eq!(summary.window_id.as_deref(), Some("thread-compact:1"));
}

#[tokio::test]
async fn isolates_threads_identities_and_activity_line_ranges() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session_a = session("thread-a");
    let session_b = session("thread-b");
    let mut a = store
        .begin(
            "identity-1",
            &session_a,
            test_meta("activity-a", "thread-a"),
            vec![message(EventPhase::Request, json!("a"))],
        )
        .await
        .expect("begin thread a");
    let a_summary = a.finish(RolloutStatus::Completed).expect("finish a");
    let mut b = store
        .begin(
            "identity-1",
            &session_b,
            test_meta("activity-b", "thread-b"),
            vec![message(EventPhase::Request, json!("b"))],
        )
        .await
        .expect("begin thread b");
    b.finish(RolloutStatus::Completed).expect("finish b");
    let mut other_identity = store
        .begin(
            "identity-2",
            &session_a,
            test_meta("activity-other", "thread-a"),
            vec![message(EventPhase::Request, json!("other"))],
        )
        .await
        .expect("begin other identity");
    other_identity
        .finish(RolloutStatus::Completed)
        .expect("finish other identity");

    assert_ne!(a_summary.relative_path, "identity-1/thread-b.jsonl");
    assert!(root.path().join("identity-1/thread-a.jsonl").exists());
    assert!(root.path().join("identity-1/thread-b.jsonl").exists());
    assert!(root.path().join("identity-2/thread-a.jsonl").exists());
    let slice = store
        .read_slice(
            &a_summary.relative_path,
            a_summary.start_line,
            a_summary.end_line,
        )
        .expect("read activity slice");
    assert_eq!(slice.event_count, a_summary.event_count);
    assert_eq!(slice.byte_size, a_summary.byte_size);
    assert_eq!(slice.sha256, a_summary.sha256);
}

#[tokio::test]
async fn repairs_an_incomplete_tail_before_appending() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-partial");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-first", "thread-partial"),
            vec![message(EventPhase::Request, json!("first user"))],
        )
        .await
        .expect("begin first activity");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first activity");

    let path = root.path().join("identity-1/thread-partial.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open thread file");
    file.write_all(br#"{"seq":4,"timestamp":"2026-10-30T00:00:00Z","type":"message","payload":"#)
        .expect("write partial tail");
    drop(file);

    let mut second = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-second", "thread-partial"),
            vec![
                message(EventPhase::Request, json!("first user")),
                message(EventPhase::Request, json!("second user")),
            ],
        )
        .await
        .expect("begin after a partial tail");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish second activity");

    let events = store
        .read_events("identity-1/thread-partial.jsonl")
        .expect("read the repaired thread");
    let contents = events
        .iter()
        .filter(|event| event.event_type == "message")
        .map(|event| event.payload["content"].clone())
        .collect::<Vec<_>>();
    assert_eq!(contents, vec![json!("first user"), json!("second user")]);
    let bytes = std::fs::read(&path).expect("read thread bytes");
    assert!(bytes.ends_with(b"\n"));
}

#[test]
fn releases_thread_locks_after_the_writer_is_dropped() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let first_path = std::path::Path::new("thread-a");
    let first = store.lock_for(first_path);
    assert_eq!(store.locks.lock().expect("lock map").len(), 1);

    drop(first);
    let _second = store.lock_for(std::path::Path::new("thread-b"));

    let locks = store.locks.lock().expect("lock map");
    assert_eq!(locks.len(), 1);
    assert!(!locks.contains_key(first_path));
}
