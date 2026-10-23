use serde_json::json;

use super::{
    EventPhase, MessageEvent, RolloutError, RolloutEventBody, RolloutStatus, RolloutStore,
    SessionMetaEvent,
};

fn test_meta(activity_id: &str) -> SessionMetaEvent {
    SessionMetaEvent {
        activity_id: activity_id.to_string(),
        session_id: "session-1".to_string(),
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

#[test]
fn writes_events_in_order_and_renames_part_file() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let mut writer = store
        .begin(
            test_meta("activity-1"),
            vec![message(EventPhase::Request, json!("hello"))],
        )
        .expect("begin rollout");

    writer
        .append(message(EventPhase::Response, json!("answer")))
        .expect("append response");
    let summary = writer
        .finish(RolloutStatus::Completed)
        .expect("finish rollout");

    assert!(summary.relative_path.ends_with(".jsonl"));
    assert!(root.path().join(&summary.relative_path).exists());
    assert!(!root
        .path()
        .join(format!("{}.part", summary.relative_path))
        .exists());

    let events = store
        .read_events(&summary.relative_path)
        .expect("read events");
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].event_type, "session_meta");
    assert_eq!(events[1].payload["content"], json!("hello"));
    assert_eq!(events[2].payload["content"], json!("answer"));
    assert_eq!(events[3].event_type, "session_end");
    assert_eq!(events[3].payload["status"], json!("completed"));
}

#[test]
fn preserves_content_larger_than_the_old_64k_limit() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let large = "x".repeat(1024 * 1024);
    let mut writer = store
        .begin(
            test_meta("activity-large"),
            vec![message(EventPhase::Request, json!(large.clone()))],
        )
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
    assert_eq!(summary.event_count, 3);
}

#[test]
fn redacts_sensitive_values_before_writing() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let mut writer = store
        .begin(
            test_meta("activity-secret"),
            vec![message(
                EventPhase::Request,
                json!("Authorization: Bearer sk-live-secret"),
            )],
        )
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
fn lists_incomplete_part_files_without_deleting_them() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let nested = root.path().join("2026/10/22");
    std::fs::create_dir_all(&nested).expect("create nested rollout directory");
    let part = nested.join("rollout-incomplete.jsonl.part");
    std::fs::write(&part, b"{}").expect("write incomplete rollout");
    std::fs::write(nested.join("rollout-complete.jsonl"), b"{}").expect("write complete rollout");

    assert_eq!(
        store.part_files().expect("list incomplete rollouts"),
        vec![part.clone()]
    );
    assert!(part.exists());
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
    let relative = "2026/10/22/rollout-partial.jsonl";
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
    let nested = root.path().join("2026/10/22");
    std::fs::create_dir_all(&nested).expect("create nested rollout directory");
    std::fs::write(nested.join("rollout-complete.jsonl"), b"{}").expect("write rollout");
    std::fs::write(nested.join("rollout-incomplete.jsonl.part"), b"{}")
        .expect("write incomplete rollout");

    assert_eq!(
        store.rollout_files().expect("list completed rollouts"),
        vec!["2026/10/22/rollout-complete.jsonl".to_string()]
    );
}

#[test]
fn finished_writer_keeps_its_persisted_status_on_repeated_finish() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let mut writer = store
        .begin(test_meta("activity-finished"), Vec::new())
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
}

#[test]
fn rejects_path_traversal_when_resolving_a_rollout_file() {
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
}

#[test]
fn reports_matching_metrics_when_reading_a_completed_rollout() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let mut writer = store
        .begin(
            test_meta("activity-verified"),
            vec![message(EventPhase::Request, json!("verified input"))],
        )
        .expect("begin verified rollout");
    let summary = writer
        .finish(RolloutStatus::Completed)
        .expect("finish verified rollout");

    let file = store
        .read_verified(&summary.relative_path)
        .expect("read verified rollout");

    assert_eq!(file.events.len(), summary.event_count as usize);
    assert_eq!(file.event_count, summary.event_count);
    assert_eq!(file.byte_size, summary.byte_size);
    assert_eq!(file.sha256, summary.sha256);
    assert_eq!(
        file.events.last().map(|event| event.event_type.as_str()),
        Some("session_end")
    );
}
