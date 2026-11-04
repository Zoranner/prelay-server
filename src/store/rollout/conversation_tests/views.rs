use std::fs;

use serde_json::{json, Value};

use super::*;
use crate::{
    domain::activity::{
        ConversationError, ConversationReasoning, ConversationToolCall, ConversationToolResult,
        ConversationUsage,
    },
    store::rollout::cache::ThreadSource,
};

#[tokio::test]
async fn read_views_share_one_content_stream_and_keep_refs() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = SessionContext::new("thread-read-views");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            turn("activity-read-views"),
            vec![
                message(EventPhase::Request, "question"),
                ContentDraft::new(ContentEventBody::Reasoning(ConversationReasoning {
                    phase: EventPhase::Request,
                    summary: Some("summary".to_string()),
                    content: None,
                })),
                ContentDraft::new(ContentEventBody::ToolCall(ConversationToolCall {
                    phase: EventPhase::Request,
                    tool_call_id: "call_1".to_string(),
                    name: "exec_command".to_string(),
                    arguments: json!({ "cmd": "ls" }),
                })),
                ContentDraft::new(ContentEventBody::ToolResult(ConversationToolResult {
                    phase: EventPhase::Request,
                    tool_call_id: "call_1".to_string(),
                    content: json!("ok"),
                })),
            ],
        )
        .await
        .expect("begin read-view thread");
    writer
        .append_content(ContentDraft::new(ContentEventBody::Message(
            ConversationMessage {
                phase: EventPhase::Response,
                role: "assistant".to_string(),
                message_phase: Some("final_answer".to_string()),
                content: json!("answer"),
            },
        )))
        .expect("append answer");
    writer
        .append_content(ContentDraft::new(ContentEventBody::Usage(
            ConversationUsage {
                input_tokens: Some(1),
                output_tokens: Some(2),
                reasoning_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
        )))
        .expect("append usage");
    writer
        .append_content(ContentDraft::new(ContentEventBody::Error(
            ConversationError {
                code: Some("test_error".to_string()),
                message: "failed".to_string(),
                http_status: Some(500),
            },
        )))
        .expect("append error");
    let summary = writer
        .finish(RolloutStatus::Failed)
        .expect("finish read-view thread");
    let file = store
        .read_verified(&summary.relative_path)
        .expect("read conversation file");

    let content = file.content_events();
    assert_eq!(
        content.len(),
        6,
        "events: {:?}",
        file.events
            .iter()
            .map(|event| &event.event_type)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        content[0].seq,
        event_seq(
            &lines(&store, &summary.relative_path),
            "message",
            "question"
        )
    );
    assert!(matches!(content[1].body, ContentEventBody::Reasoning(_)));
    assert!(matches!(content[2].body, ContentEventBody::ToolCall(_)));
    assert!(matches!(content[3].body, ContentEventBody::ToolResult(_)));
    assert!(matches!(content[4].body, ContentEventBody::Message(_)));
    assert!(matches!(content[5].body, ContentEventBody::Error(_)));

    let memory = file.memory_events();
    assert_eq!(memory.len(), 5);
    assert!(!memory
        .iter()
        .any(|event| matches!(event.body, ContentEventBody::Error(_))));

    let lines = lines(&store, &summary.relative_path);
    let turn_id = lines
        .iter()
        .find(|line| line["type"] == json!("turn"))
        .and_then(|line| line["payload"]["turn_id"].as_str())
        .expect("turn id");
    let refs = file
        .context_refs_for_turn(turn_id)
        .expect("context refs for turn");
    assert_eq!(refs.len(), 4);
    assert_eq!(file.resolve_refs(&refs).len(), 4);

    let index =
        fs::read_to_string(store.absolute_path(&format!("{}.index", summary.relative_path)))
            .expect("read conversation index");
    assert!(!index.contains("question"));
    assert!(!index.contains("answer"));
    assert!(!index.contains("failed"));
}

#[tokio::test]
async fn invalidates_an_old_index_and_rebuilds_it() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = SessionContext::new("thread-index-version");
    let mut writer = store
        .begin(
            "identity-1",
            &session,
            turn("activity-index-version"),
            vec![message(EventPhase::Request, "first")],
        )
        .await
        .expect("begin indexed thread");
    writer
        .finish(RolloutStatus::Completed)
        .expect("finish indexed thread");
    let index_path = store.absolute_path("identity-1/thread-index-version.jsonl.index");
    let mut index = serde_json::from_slice::<Value>(&fs::read(&index_path).expect("read index"))
        .expect("parse index");
    index["version"] = json!(2);
    fs::write(
        &index_path,
        serde_json::to_vec(&index).expect("serialize old index"),
    )
    .expect("write old index");

    let restarted = RolloutStore::new(root.path().to_path_buf());
    let loaded = restarted
        .resolve_thread_source("identity-1", "thread-index-version")
        .expect("load old-index thread");
    assert!(matches!(loaded, ThreadSource::Loaded(_, _)));
    let rebuilt =
        serde_json::from_slice::<Value>(&fs::read(&index_path).expect("read rebuilt index"))
            .expect("parse rebuilt index");
    assert_eq!(rebuilt["version"], json!(3));
}

#[tokio::test]
async fn appends_v1_events_to_a_legacy_thread_without_rewriting_its_prefix() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let relative = "identity-1/thread-legacy-append.jsonl";
    let path = store.absolute_path(relative);
    fs::create_dir_all(path.parent().expect("thread parent")).expect("create thread directory");
    fs::write(
        &path,
        concat!(
            "{\"seq\":1,\"timestamp\":\"2026-10-22T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{}}\n",
            "{\"seq\":2,\"timestamp\":\"2026-10-22T00:00:01Z\",\"type\":\"window_start\",\"payload\":{\"generation\":0}}\n",
            "{\"seq\":3,\"timestamp\":\"2026-10-22T00:00:02Z\",\"type\":\"message\",\"payload\":{\"phase\":\"request\",\"role\":\"user\",\"content\":\"old\"}}\n",
        ),
    )
    .expect("write legacy thread");

    let mut writer = store
        .begin(
            "identity-1",
            &SessionContext::new("thread-legacy-append"),
            turn("activity-legacy-append"),
            vec![message(EventPhase::Request, "new")],
        )
        .await
        .expect("append to legacy thread");
    writer
        .finish(RolloutStatus::Completed)
        .expect("finish legacy append");

    let contents = fs::read_to_string(&path).expect("read appended thread");
    assert!(contents.starts_with("{\"seq\":1"));
    let file = store.read_verified(relative).expect("read appended thread");
    assert_eq!(file.format, ConversationFormat::Legacy);
    assert!(file.events.iter().any(|event| {
        event.event_type == "message" && event.payload["content"] == json!("old")
    }));
    assert!(file.events.iter().any(|event| {
        event.event_type == "message" && event.payload["content"] == json!("new")
    }));
}
