use serde_json::json;

use super::RolloutStore;
use crate::domain::activity::rollout::{RolloutError, RolloutStatus};
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
async fn appends_events_to_one_thread_file_with_turn_end() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-1");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-1", "thread-1"),
            vec![message(EventPhase::Request, json!("hello"))],
        )
        .await
        .expect("begin rollout");

    writer
        .append_content(message(EventPhase::Response, json!("answer")))
        .expect("append response");
    let summary = writer
        .finish(RolloutStatus::Completed)
        .expect("finish rollout");

    assert_eq!(summary.relative_path, "identity-1/thread-1.jsonl");
    assert_eq!(summary.thread_id, "thread-1");
    assert_eq!(summary.start_line, 1);
    assert_eq!(summary.end_line, 7);
    assert!(root.path().join(&summary.relative_path).exists());
    assert!(!root
        .path()
        .join(format!("{}.part", summary.relative_path))
        .exists());

    let events = store
        .read_events(&summary.relative_path)
        .expect("read events");
    assert_eq!(events.len(), 6);
    assert_eq!(events[0].event_type, "window");
    assert_eq!(events[1].event_type, "turn");
    assert_eq!(events[2].payload["content"], json!("hello"));
    assert_eq!(events[3].event_type, "context");
    assert_eq!(events[4].payload["content"], json!("answer"));
    assert_eq!(events[5].event_type, "turn_end");
}

#[tokio::test]
async fn preserves_content_larger_than_the_old_64k_limit() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-large");
    let large = "x".repeat(1024 * 1024);
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-large", "thread-large"),
            vec![message(EventPhase::Request, json!(large.clone()))],
        )
        .await
        .expect("begin large rollout");
    let summary = writer
        .finish(RolloutStatus::Completed)
        .expect("finish large rollout");
    let events = store
        .read_events(&summary.relative_path)
        .expect("read large events");
    let message = events
        .iter()
        .find(|event| event.event_type == "message")
        .expect("request message");
    assert_eq!(message.payload["content"], json!(large));
    assert!(summary.byte_size > 1024 * 1024);
    assert_eq!(summary.event_count, 6);
}

#[tokio::test]
async fn redacts_sensitive_values_before_writing() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-secret");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            test_meta("activity-secret", "thread-secret"),
            vec![message(
                EventPhase::Request,
                json!("Authorization: Bearer sk-live-secret"),
            )],
        )
        .await
        .expect("begin rollout");
    let summary = writer
        .finish(RolloutStatus::Completed)
        .expect("finish rollout");
    let events = store
        .read_events(&summary.relative_path)
        .expect("read events");
    let serialized = serde_json::to_string(&events).expect("serialize events");
    assert!(!serialized.contains("sk-live-secret"));
    assert!(!serialized.contains("Bearer sk-live-secret"));
    assert!(serialized.contains("[REDACTED]"));
}

#[test]
fn lists_jsonl_files_with_incomplete_tails() {
    let root = tempfile::tempdir().expect("temporary rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let nested = root.path().join("identity-1");
    std::fs::create_dir_all(&nested).expect("create rollout directory");
    std::fs::write(nested.join("complete.jsonl"), b"{}\n").expect("write complete thread");
    std::fs::write(nested.join("partial.jsonl"), b"{\"seq\":1").expect("write partial thread");
    std::fs::write(nested.join("legacy.jsonl.part"), b"{}").expect("write legacy part");

    let files = store
        .incomplete_rollout_files()
        .expect("list incomplete thread files");

    assert_eq!(files, vec![nested.join("partial.jsonl")]);
}

#[test]
fn validates_a_writable_rollout_root() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().join("nested/sessions"));

    store.validate().expect("writable rollout root");

    let remaining = std::fs::read_dir(root.path().join("nested/sessions"))
        .expect("read rollout root")
        .collect::<Result<Vec<_>, _>>()
        .expect("read rollout entries");
    assert!(
        remaining.is_empty(),
        "write probe must be removed after validation"
    );
}

#[test]
fn rejects_a_rollout_root_that_is_a_file() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let file_path = root.path().join("sessions");
    std::fs::write(&file_path, b"not a directory").expect("write file root");
    let store = RolloutStore::new(file_path);

    assert!(store.validate().is_err());
}

#[test]
fn reports_an_incomplete_last_line_with_the_valid_prefix_length() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let relative = "identity-1/thread-partial.jsonl";
    let path = store.absolute_path(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(
        &path,
        concat!(
            "{\"seq\":1,\"timestamp\":\"2026-10-22T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{}}\n",
            "{\"seq\":2,\"timestamp\":\"2026-10-22T00:00:01Z\""
        ),
    )
    .expect("write partial rollout");

    match store.read_events(relative) {
        Err(RolloutError::PartialLine {
            valid_events,
            path: error_path,
        }) => {
            assert_eq!(valid_events, 1);
            assert_eq!(error_path, path);
        }
        other => panic!("expected a partial line error, got {other:?}"),
    }
}

#[test]
fn lists_completed_rollout_files_as_posix_relative_paths() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let nested = root.path().join("identity-1");
    std::fs::create_dir_all(&nested).expect("create nested rollout directory");
    std::fs::write(nested.join("thread-complete.jsonl"), b"{}").expect("write rollout");
    std::fs::write(nested.join("thread-incomplete.jsonl.part"), b"{}")
        .expect("write incomplete rollout");

    assert_eq!(
        store.rollout_files().expect("list completed rollouts"),
        vec!["identity-1/thread-complete.jsonl".to_string()]
    );
}

#[test]
fn finished_writer_keeps_its_persisted_status_on_repeated_finish() {
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    runtime.block_on(async {
        let root = tempfile::tempdir().expect("temp rollout root");
        let store = RolloutStore::new(root.path().to_path_buf());
        let session = session("thread-finished");
        let mut writer = store
            .begin(
                "identity-1",
                &session,
                test_meta("activity-finished", "thread-finished"),
                Vec::new(),
            )
            .await
            .expect("begin rollout");

        let first = writer
            .finish(RolloutStatus::Failed)
            .expect("finish failed rollout");
        let second = writer
            .finish(RolloutStatus::Completed)
            .expect("repeat finish");

        assert_eq!(first.status, RolloutStatus::Failed);
        assert_eq!(second.status, RolloutStatus::Failed);
        assert_eq!(second.relative_path, first.relative_path);
    });
}

#[test]
fn rejects_path_traversal_and_unsafe_thread_segments() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());

    for path in [
        "../outside.jsonl",
        "2026/../../outside.jsonl",
        "/tmp/x.jsonl",
    ] {
        assert!(matches!(
            store.checked_absolute_path(path),
            Err(RolloutError::InvalidPath(_))
        ));
    }

    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    runtime.block_on(async {
        let unsafe_session = session("../outside");
        assert!(matches!(
            store
                .begin(
                    "identity-1",
                    &unsafe_session,
                    test_meta("activity-unsafe", "../outside"),
                    Vec::new(),
                )
                .await,
            Err(RolloutError::InvalidPath(_))
        ));
    });
}
