use std::io::Write;

use serde_json::json;

use super::cache::ThreadSource;
use super::RolloutStore;
use crate::domain::activity::rollout::RolloutStatus;
use crate::domain::activity::{
    ContentDraft, ContentEventBody, ConversationMessage, EventPhase, SessionContext, TurnContext,
};

fn test_meta(activity_id: &str, _session_id: &str) -> TurnContext {
    TurnContext {
        activity_id: activity_id.to_string(),
        model_requested: "deepseek-flash".to_string(),
        model_upstream: "deepseek/deepseek-v4.1-flash".to_string(),
    }
}

fn message(phase: EventPhase, content: serde_json::Value) -> ContentDraft {
    ContentDraft::new(ContentEventBody::Message(ConversationMessage {
        phase,
        role: "user".to_string(),
        message_phase: None,
        content,
    }))
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

    std::fs::remove_file(root.path().join("identity-1/thread-cache.jsonl.index"))
        .expect("remove persisted index");
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
async fn reuses_a_persisted_thread_index_after_restart() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-persisted");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-persisted", "thread-persisted"),
            vec![message(EventPhase::Request, json!("first"))],
        )
        .await
        .expect("begin persisted thread");
    writer
        .finish(RolloutStatus::Completed)
        .expect("finish persisted thread");

    let restarted = RolloutStore::new(root.path().to_path_buf());
    let loaded = restarted
        .resolve_thread_source("identity-1", "thread-persisted")
        .expect("load persisted thread");

    let ThreadSource::Cached(state) = loaded else {
        panic!("expected the persisted index to be reused");
    };
    assert_eq!(state.relative_path, "identity-1/thread-persisted.jsonl");
    assert_eq!(state.line_count, 6);
    assert_eq!(state.event_refs.values().map(Vec::len).sum::<usize>(), 1);

    let mut resumed = restarted
        .begin(
            "identity-1",
            &session,
            test_meta("activity-persisted-second", "thread-persisted"),
            vec![
                message(EventPhase::Request, json!("first")),
                message(EventPhase::Request, json!("second")),
            ],
        )
        .await
        .expect("resume persisted thread");
    resumed
        .finish(RolloutStatus::Completed)
        .expect("finish resumed thread");
    let contents = restarted
        .read_events("identity-1/thread-persisted.jsonl")
        .expect("read resumed thread")
        .into_iter()
        .filter(|event| event.event_type == "message")
        .map(|event| event.payload["content"].clone())
        .collect::<Vec<_>>();
    assert_eq!(contents, vec![json!("first"), json!("second")]);
}

#[tokio::test]
async fn reuses_a_persisted_index_for_a_recovered_segment() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-recovered-index");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-recovered-first", "thread-recovered-index"),
            vec![message(EventPhase::Request, json!("first"))],
        )
        .await
        .expect("begin base thread");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish base thread");

    let base_path = root.path().join("identity-1/thread-recovered-index.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&base_path)
        .expect("open base thread");
    writeln!(file, "{{not-json}}").expect("write corrupt line");
    drop(file);

    let mut recovered = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-recovered-second", "thread-recovered-index"),
            vec![message(EventPhase::Request, json!("second"))],
        )
        .await
        .expect("begin recovered segment");
    let summary = recovered
        .finish(RolloutStatus::Completed)
        .expect("finish recovered segment");
    assert!(summary
        .relative_path
        .ends_with("thread-recovered-index.recovered.jsonl"));

    let restarted = RolloutStore::new(root.path().to_path_buf());
    let loaded = restarted
        .resolve_thread_source("identity-1", "thread-recovered-index")
        .expect("load recovered index");

    let ThreadSource::Cached(state) = loaded else {
        panic!("expected the recovered segment index to be reused");
    };
    assert_eq!(state.relative_path, summary.relative_path);
}

#[tokio::test]
async fn falls_back_to_a_full_read_when_the_persisted_index_is_missing() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-index-missing");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-index-missing", "thread-index-missing"),
            vec![message(EventPhase::Request, json!("first"))],
        )
        .await
        .expect("begin thread");
    writer
        .finish(RolloutStatus::Completed)
        .expect("finish thread");

    std::fs::remove_file(
        root.path()
            .join("identity-1/thread-index-missing.jsonl.index"),
    )
    .expect("remove persisted thread index");

    let restarted = RolloutStore::new(root.path().to_path_buf());
    let loaded = restarted
        .resolve_thread_source("identity-1", "thread-index-missing")
        .expect("load thread");

    assert!(matches!(loaded, ThreadSource::Loaded(_, _)));
}

#[tokio::test]
async fn ignores_a_stale_persisted_index_after_an_external_append() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-stale-index");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-stale-index", "thread-stale-index"),
            vec![message(EventPhase::Request, json!("first"))],
        )
        .await
        .expect("begin thread");
    writer
        .finish(RolloutStatus::Completed)
        .expect("finish thread");

    let thread_path = root.path().join("identity-1/thread-stale-index.jsonl");
    let external_event = json!({
        "seq": 99,
        "timestamp": "2026-11-03T00:00:00Z",
        "type": "message",
        "payload": {
            "phase": "request",
            "role": "user",
            "content": "external"
        }
    });
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&thread_path)
        .expect("open thread for external append");
    writeln!(file, "{external_event}").expect("append external event");
    drop(file);

    let restarted = RolloutStore::new(root.path().to_path_buf());
    let loaded = restarted
        .resolve_thread_source("identity-1", "thread-stale-index")
        .expect("load stale-index thread");

    assert!(matches!(loaded, ThreadSource::Loaded(_, _)));
}

#[tokio::test]
async fn falls_back_to_a_full_read_when_the_persisted_index_is_corrupt() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-corrupt-index");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-corrupt-index", "thread-corrupt-index"),
            vec![message(EventPhase::Request, json!("first"))],
        )
        .await
        .expect("begin thread");
    writer
        .finish(RolloutStatus::Completed)
        .expect("finish thread");
    std::fs::write(
        root.path()
            .join("identity-1/thread-corrupt-index.jsonl.index"),
        b"{not-json",
    )
    .expect("corrupt persisted thread index");

    let restarted = RolloutStore::new(root.path().to_path_buf());
    let loaded = restarted
        .resolve_thread_source("identity-1", "thread-corrupt-index")
        .expect("load thread");

    assert!(matches!(loaded, ThreadSource::Loaded(_, _)));
}

#[tokio::test]
async fn skips_the_index_when_the_thread_changed_during_writing() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-concurrent-write");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-concurrent-write", "thread-concurrent-write"),
            vec![message(EventPhase::Request, json!("first"))],
        )
        .await
        .expect("begin thread");
    let mut external = std::fs::OpenOptions::new()
        .append(true)
        .open(writer.final_path())
        .expect("open external writer");
    external
        .write_all(
            br#"{"seq":99,"timestamp":"2026-11-03T00:00:00Z","type":"message","payload":{"phase":"request","role":"user","content":"external"}}"#,
        )
        .expect("append external event");
    external.write_all(b"\n").expect("terminate external event");
    drop(external);

    writer
        .finish(RolloutStatus::Completed)
        .expect("finish thread");

    assert!(!root
        .path()
        .join("identity-1/thread-concurrent-write.jsonl.index")
        .exists());
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
    std::fs::remove_file(root.path().join("identity-1/thread-forget.jsonl.index"))
        .expect("remove persisted index");

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
        .append_content(message(EventPhase::Response, json!("first answer")))
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
