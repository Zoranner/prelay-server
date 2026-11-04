use std::fs;

use serde_json::{json, Value};

use super::RolloutStore;
use crate::domain::activity::{
    ContentDraft, ContentEventBody, ConversationFormat, ConversationMessage, EventPhase,
    RolloutStatus, SessionContext, TurnContext,
};

mod views;

fn turn(activity_id: &str) -> TurnContext {
    TurnContext {
        activity_id: activity_id.to_string(),
        model_requested: "deepseek-flash".to_string(),
        model_upstream: "deepseek/deepseek-v4.1-flash".to_string(),
    }
}

fn message(phase: EventPhase, content: &str) -> ContentDraft {
    ContentDraft::new(ContentEventBody::Message(ConversationMessage {
        phase,
        role: "user".to_string(),
        message_phase: None,
        content: json!(content),
    }))
}

fn lines(store: &RolloutStore, relative: &str) -> Vec<Value> {
    fs::read_to_string(store.absolute_path(relative))
        .expect("read thread file")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse thread line"))
        .collect()
}

fn event_seq(lines: &[Value], event_type: &str, content: &str) -> u64 {
    lines
        .iter()
        .find(|line| {
            line["type"] == json!(event_type) && line["payload"]["content"] == json!(content)
        })
        .and_then(|line| line["seq"].as_u64())
        .expect("matching event seq")
}

#[tokio::test]
async fn writes_v1_header_turn_context_and_turn_end_once() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = SessionContext::new("thread-v1");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            turn("activity-1"),
            vec![message(EventPhase::Request, "hello")],
        )
        .await
        .expect("begin v1 thread");
    writer
        .append_content(message(EventPhase::Response, "answer"))
        .expect("append answer");
    let summary = writer
        .finish(RolloutStatus::Completed)
        .expect("finish v1 thread");

    let lines = lines(&store, &summary.relative_path);
    assert_eq!(lines[0]["v"], json!(1));
    assert_eq!(lines[0]["type"], json!("meta"));
    assert_eq!(lines[0]["thread_id"], json!("thread-v1"));
    assert!(lines[0].get("seq").is_none());
    assert!(lines[0]["created_at"]
        .as_str()
        .expect("meta created at")
        .ends_with("+08:00"));
    assert!(lines[0].get("timestamp").is_none());
    assert_eq!(
        lines
            .iter()
            .filter(|line| line["type"] == json!("meta"))
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line["type"] == json!("turn"))
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line["type"] == json!("turn_end"))
            .count(),
        1
    );
    for line in lines.iter().filter(|line| line.get("seq").is_some()) {
        assert!(line["time"]
            .as_str()
            .expect("event time")
            .ends_with("+08:00"));
    }
    let request_seq = event_seq(&lines, "message", "hello");
    let context = lines
        .iter()
        .find(|line| line["type"] == json!("context"))
        .expect("context event");
    assert_eq!(context["payload"]["refs"], json!([request_seq]));
    let turn_end = lines
        .iter()
        .find(|line| line["type"] == json!("turn_end"))
        .expect("turn end");
    assert_eq!(turn_end["payload"]["status"], json!("completed"));

    let file = store
        .read_verified(&summary.relative_path)
        .expect("read v1 thread");
    assert_eq!(file.line_count, lines.len() as u64);
    assert_eq!(file.event_count as usize, file.events.len());
    assert_eq!(summary.event_count, file.event_count);
}

#[tokio::test]
async fn history_replay_only_appends_context_refs() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = SessionContext::new("thread-history");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            turn("activity-first"),
            vec![message(EventPhase::Request, "first")],
        )
        .await
        .expect("begin first activity");
    first
        .append_content(message(EventPhase::Response, "answer"))
        .expect("append first answer");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first activity");

    let mut second = store
        .begin(
            "identity-1",
            &session,
            turn("activity-second"),
            vec![
                message(EventPhase::Request, "first"),
                message(EventPhase::Request, "answer"),
                message(EventPhase::Request, "second"),
            ],
        )
        .await
        .expect("begin replay activity");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish replay activity");

    let lines = lines(&store, "identity-1/thread-history.jsonl");
    let messages = lines
        .iter()
        .filter(|line| line["type"] == json!("message"))
        .map(|line| line["payload"]["content"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        vec![json!("first"), json!("answer"), json!("second")]
    );
    let contexts = lines
        .iter()
        .filter(|line| line["type"] == json!("context"))
        .map(|line| line["payload"]["refs"].clone())
        .collect::<Vec<_>>();
    assert_eq!(contexts[0], json!([event_seq(&lines, "message", "first")]));
    assert_eq!(contexts[1].as_array().expect("second context").len(), 3);
}

#[tokio::test]
async fn repeated_real_messages_append_new_content_events() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = SessionContext::new("thread-repeat");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            turn("activity-first"),
            vec![message(EventPhase::Request, "repeat")],
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
            turn("activity-second"),
            vec![
                message(EventPhase::Request, "repeat"),
                message(EventPhase::Request, "repeat"),
            ],
        )
        .await
        .expect("begin repeated activity");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish repeated activity");

    let lines = lines(&store, "identity-1/thread-repeat.jsonl");
    assert_eq!(
        lines
            .iter()
            .filter(|line| {
                line["type"] == json!("message") && line["payload"]["content"] == json!("repeat")
            })
            .count(),
        2
    );
    let contexts = lines
        .iter()
        .filter(|line| line["type"] == json!("context"))
        .collect::<Vec<_>>();
    let refs = contexts[1]["payload"]["refs"]
        .as_array()
        .expect("repeated context refs");
    assert_eq!(refs.len(), 2);
    assert_ne!(refs[0], refs[1]);
}

#[tokio::test]
async fn window_change_resets_counts_but_stable_ids_can_reference_old_content() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let first_session = SessionContext::new("thread-window");
    let mut first = store
        .begin(
            "identity-1",
            &first_session,
            turn("activity-first"),
            vec![message(EventPhase::Request, "stable").with_item_id("msg_stable")],
        )
        .await
        .expect("begin first window");
    first
        .finish(RolloutStatus::Completed)
        .expect("finish first window");

    let compacted =
        SessionContext::with_window("thread-window", Some("thread-window:1".to_string()), 1);
    let mut second = store
        .begin(
            "identity-1",
            &compacted,
            turn("activity-second"),
            vec![message(EventPhase::Request, "stable").with_item_id("msg_stable")],
        )
        .await
        .expect("begin compacted window");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish compacted window");

    let lines = lines(&store, "identity-1/thread-window.jsonl");
    assert_eq!(
        lines
            .iter()
            .filter(|line| line["type"] == json!("window"))
            .count(),
        2
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| {
                line["type"] == json!("message") && line["payload"]["content"] == json!("stable")
            })
            .count(),
        1
    );
    let contexts = lines
        .iter()
        .filter(|line| line["type"] == json!("context"))
        .collect::<Vec<_>>();
    assert_eq!(
        contexts[1]["payload"]["refs"],
        contexts[0]["payload"]["refs"]
    );
}

#[tokio::test]
async fn reads_legacy_files_without_fabricating_missing_fields() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let relative = "identity-1/thread-legacy.jsonl";
    let path = store.absolute_path(relative);
    fs::create_dir_all(path.parent().expect("thread parent")).expect("create thread directory");
    fs::write(
        &path,
        concat!(
            "{\"seq\":1,\"timestamp\":\"2026-10-22T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"activity_id\":\"old\"}}\n",
            "{\"seq\":2,\"timestamp\":\"2026-10-22T00:00:01Z\",\"type\":\"window_start\",\"payload\":{\"generation\":0}}\n",
            "{\"seq\":3,\"timestamp\":\"2026-10-22T00:00:02Z\",\"type\":\"message\",\"payload\":{\"phase\":\"request\",\"role\":\"user\",\"content\":\"old text\"}}\n",
            "{\"seq\":4,\"timestamp\":\"2026-10-22T00:00:03Z\",\"type\":\"session_end\",\"payload\":{\"status\":\"completed\"}}\n",
        ),
    )
    .expect("write legacy thread");

    let file = store.read_verified(relative).expect("read legacy thread");
    assert_eq!(file.format, ConversationFormat::Legacy);
    assert!(file.meta.is_none());
    assert!(file.events.iter().any(|event| {
        event.event_type == "message" && event.payload["content"] == json!("old text")
    }));
    assert!(!serde_json::to_string(&file.events)
        .expect("serialize legacy events")
        .contains("session_meta"));
    assert!(file
        .events
        .iter()
        .any(|event| event.event_type == "turn_end"));
}

#[tokio::test]
async fn reordered_history_reuses_existing_content_refs() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = SessionContext::new("thread-reordered");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            turn("activity-first"),
            vec![
                message(EventPhase::Request, "first"),
                message(EventPhase::Request, "second"),
            ],
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
            turn("activity-second"),
            vec![
                message(EventPhase::Request, "third"),
                message(EventPhase::Request, "first"),
                message(EventPhase::Request, "second"),
            ],
        )
        .await
        .expect("begin reordered activity");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish reordered activity");

    let lines = lines(&store, "identity-1/thread-reordered.jsonl");
    let contexts = lines
        .iter()
        .filter(|line| line["type"] == json!("context"))
        .collect::<Vec<_>>();
    let refs = contexts[1]["payload"]["refs"]
        .as_array()
        .expect("reordered refs");
    assert_eq!(refs.len(), 3);
    assert_eq!(refs[1], json!(event_seq(&lines, "message", "first")));
    assert_eq!(refs[2], json!(event_seq(&lines, "message", "second")));
}

#[tokio::test]
async fn history_without_system_prefix_appends_only_new_user_content() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = SessionContext::new("thread-system");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            turn("activity-first"),
            vec![
                ContentDraft::new(ContentEventBody::Message(ConversationMessage {
                    phase: EventPhase::Request,
                    role: "system".to_string(),
                    message_phase: None,
                    content: json!("rules"),
                })),
                message(EventPhase::Request, "hello"),
            ],
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
            turn("activity-second"),
            vec![
                message(EventPhase::Request, "hello"),
                message(EventPhase::Request, "next"),
            ],
        )
        .await
        .expect("begin history without system prefix");
    second
        .finish(RolloutStatus::Completed)
        .expect("finish history without system prefix");

    let lines = lines(&store, "identity-1/thread-system.jsonl");
    assert_eq!(
        lines
            .iter()
            .filter(|line| {
                line["type"] == json!("message") && line["payload"]["content"] == json!("hello")
            })
            .count(),
        1
    );
    assert!(lines.iter().any(|line| {
        line["type"] == json!("message") && line["payload"]["content"] == json!("next")
    }));
}
