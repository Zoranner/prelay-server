use serde_json::json;

use super::{
    CompletedTurnMaterialization, FinishedTurnMaterialization, RolloutStore,
    StartedTurnMaterialization, TimedContentDraft,
};
use crate::domain::activity::{
    ContentDraft, ContentEventBody, ConversationMessage, EventPhase, RolloutStatus, SessionContext,
    TurnContext,
};

fn session(thread_id: &str) -> SessionContext {
    SessionContext::new(thread_id)
}

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

fn completed_materialization() -> CompletedTurnMaterialization {
    CompletedTurnMaterialization {
        session: session("thread-materialize"),
        turn: turn("activity-materialize"),
        created_at: "2026-11-17T12:00:00.000+08:00".to_string(),
        request_events: vec![TimedContentDraft::new(
            "2026-11-17T12:00:00.100+08:00",
            message(EventPhase::Request, "hello"),
        )],
        response_events: vec![TimedContentDraft::new(
            "2026-11-17T12:00:00.900+08:00",
            message(EventPhase::Response, "answer"),
        )],
        status: RolloutStatus::Completed,
        http_status: Some(200),
        error_code: None,
        error_message: None,
        finished_at: "2026-11-17T12:00:01.000+08:00".to_string(),
    }
}

fn started_materialization() -> StartedTurnMaterialization {
    StartedTurnMaterialization {
        session: session("thread-stream"),
        turn: turn("activity-stream"),
        created_at: "2026-11-17T12:00:00.000+08:00".to_string(),
        request_events: vec![TimedContentDraft::new(
            "2026-11-17T12:00:00.100+08:00",
            message(EventPhase::Request, "hello"),
        )],
    }
}

fn finished_materialization() -> FinishedTurnMaterialization {
    FinishedTurnMaterialization {
        session: session("thread-stream"),
        activity_id: "activity-stream".to_string(),
        response_events: vec![TimedContentDraft::new(
            "2026-11-17T12:00:00.900+08:00",
            message(EventPhase::Response, "answer"),
        )],
        status: RolloutStatus::Completed,
        http_status: Some(200),
        error_code: None,
        error_message: None,
        finished_at: "2026-11-17T12:00:01.000+08:00".to_string(),
    }
}

#[tokio::test]
async fn finds_each_activity_turn_with_its_persisted_range_and_summary() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-lookup");
    let mut first = store
        .begin(
            "identity-1",
            &session,
            turn("activity-first"),
            vec![message(EventPhase::Request, "first")],
        )
        .await
        .expect("begin first turn");
    let first_summary = first
        .finish(RolloutStatus::Completed)
        .expect("finish first turn");
    let mut second = store
        .begin(
            "identity-1",
            &session,
            turn("activity-second"),
            vec![message(EventPhase::Request, "second")],
        )
        .await
        .expect("begin second turn");
    let second_summary = second
        .finish(RolloutStatus::Completed)
        .expect("finish second turn");

    let found_first = store
        .find_activity_turn("identity-1", "thread-lookup", "activity-first")
        .expect("find first turn")
        .expect("first turn exists");
    assert!(found_first.complete);
    assert_eq!(found_first.summary("activity-first"), first_summary);
    let found_second = store
        .find_activity_turn("identity-1", "thread-lookup", "activity-second")
        .expect("find second turn")
        .expect("second turn exists");
    assert!(found_second.complete);
    assert_eq!(found_second.summary("activity-second"), second_summary);
    assert!(store
        .find_activity_turn("identity-1", "thread-lookup", "activity-missing")
        .expect("find missing turn")
        .is_none());
}

#[tokio::test]
async fn reports_a_turn_without_turn_end_as_writing() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-incomplete");
    let mut writer = store
        .begin_at(
            "identity-1",
            &session,
            turn("activity-incomplete"),
            "2026-11-17T12:00:00.000+08:00".to_string(),
            vec![TimedContentDraft::new(
                "2026-11-17T12:00:00.100+08:00",
                message(EventPhase::Request, "incomplete"),
            )],
        )
        .await
        .expect("begin incomplete turn");
    writer
        .append_content_at(
            message(EventPhase::Response, "partial"),
            "2026-11-17T12:00:00.500+08:00",
        )
        .expect("append partial response");
    drop(writer);

    let found = store
        .find_activity_turn("identity-1", "thread-incomplete", "activity-incomplete")
        .expect("find incomplete turn")
        .expect("incomplete turn exists");
    assert!(!found.complete);
    assert_eq!(found.status, RolloutStatus::Writing);
}

#[tokio::test]
async fn materializing_a_completed_turn_twice_does_not_append_it_twice() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let first = store
        .materialize_completed_turn("identity-1", completed_materialization())
        .await
        .expect("first materialization");
    let second = store
        .materialize_completed_turn("identity-1", completed_materialization())
        .await
        .expect("second materialization");

    assert_eq!(first, second);
    let events = store
        .read_events("identity-1/thread-materialize.jsonl")
        .expect("read materialized thread");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "turn")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.payload["content"] == json!("answer"))
            .count(),
        1
    );
}

#[tokio::test]
async fn refuses_to_materialize_over_an_incomplete_turn() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-materialize");
    let writer = store
        .begin_at(
            "identity-1",
            &session,
            turn("activity-materialize"),
            "2026-11-17T12:00:00.000+08:00".to_string(),
            vec![TimedContentDraft::new(
                "2026-11-17T12:00:00.100+08:00",
                message(EventPhase::Request, "hello"),
            )],
        )
        .await
        .expect("begin incomplete turn");
    drop(writer);

    assert!(store
        .materialize_completed_turn("identity-1", completed_materialization())
        .await
        .is_err());
}

#[tokio::test]
async fn materializing_a_started_turn_twice_does_not_duplicate_request_events() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let first = store
        .materialize_started_turn("identity-1", started_materialization())
        .await
        .expect("first start");
    let second = store
        .materialize_started_turn("identity-1", started_materialization())
        .await
        .expect("second start");
    assert_eq!(first, second);
    assert!(!first.complete);
    let events = store
        .read_events("identity-1/thread-stream.jsonl")
        .expect("read started thread");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "turn")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.payload["content"] == json!("hello"))
            .count(),
        1
    );
}

#[tokio::test]
async fn finishes_a_started_turn_and_is_idempotent() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    store
        .materialize_started_turn("identity-1", started_materialization())
        .await
        .expect("start turn");
    let first = store
        .materialize_finished_turn("identity-1", finished_materialization())
        .await
        .expect("finish turn");
    let second = store
        .materialize_finished_turn("identity-1", finished_materialization())
        .await
        .expect("finish turn again");
    assert_eq!(first, second);
    assert_eq!(first.status, RolloutStatus::Completed);
    let events = store
        .read_events("identity-1/thread-stream.jsonl")
        .expect("read finished thread");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.payload["content"] == json!("answer"))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "turn_end")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .find(|event| event.event_type == "turn_end")
            .expect("turn end")
            .time,
        "2026-11-17T12:00:01.000+08:00"
    );
}

#[tokio::test]
async fn refuses_to_finish_a_turn_that_was_never_started() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    assert!(store
        .materialize_finished_turn("identity-1", finished_materialization())
        .await
        .is_err());
}

fn write_thread_file(root: &std::path::Path, thread_id: &str, lines: &[serde_json::Value]) {
    let directory = root.join("identity-1");
    std::fs::create_dir_all(&directory).expect("create thread directory");
    let mut contents = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    contents.push('\n');
    std::fs::write(directory.join(format!("{thread_id}.jsonl")), contents)
        .expect("write thread file");
}

#[tokio::test]
async fn a_new_turn_after_a_legacy_thread_is_not_reported_complete() {
    let root = tempfile::tempdir().expect("temp rollout root");
    // 旧格式线程文件里被忽略的控制行不会出现在解析后的事件序列里，
    // 因此事件下标与文件行号并不一一对应。
    write_thread_file(
        root.path(),
        "thread-legacy",
        &[
            json!({"seq":1,"time":"2026-11-17T12:00:00.000+08:00","type":"session_meta","payload":{"thread_id":"thread-legacy"}}),
            json!({"seq":2,"time":"2026-11-17T12:00:00.500+08:00","type":"media","payload":{"kind":"image"}}),
            json!({"seq":3,"time":"2026-11-17T12:00:01.000+08:00","type":"window_start","payload":{"generation":0}}),
            json!({"seq":4,"time":"2026-11-17T12:00:02.000+08:00","type":"turn","payload":{"turn_id":"turn-a","activity_id":"activity-a","model":"model-1","upstream_model":"model-upstream"}}),
            json!({"seq":5,"time":"2026-11-17T12:00:03.000+08:00","type":"session_end","payload":{"status":"completed"}}),
        ],
    );
    let store = RolloutStore::new(root.path().to_path_buf());
    let writer = store
        .begin(
            "identity-1",
            &session("thread-legacy"),
            turn("activity-b"),
            vec![message(EventPhase::Request, "next")],
        )
        .await
        .expect("begin turn after legacy thread");
    drop(writer);

    let found = store
        .find_activity_turn("identity-1", "thread-legacy", "activity-b")
        .expect("find new turn")
        .expect("new turn exists");
    assert!(
        !found.complete,
        "a freshly started turn must not inherit the previous turn's turn_end"
    );
}

#[tokio::test]
async fn a_turn_end_belongs_only_to_the_turn_it_closes() {
    let root = tempfile::tempdir().expect("temp rollout root");
    // 同一线程上两个请求交错：b 在 a 结束前已经开始，a 的 turn_end 落在 b 之后。
    write_thread_file(
        root.path(),
        "thread-interleaved",
        &[
            json!({"v":1,"type":"meta","thread_id":"thread-interleaved","created_at":"2026-11-17T12:00:00.000+08:00"}),
            json!({"seq":1,"time":"2026-11-17T12:00:00.100+08:00","type":"window_start","payload":{"generation":0}}),
            json!({"seq":2,"time":"2026-11-17T12:00:01.000+08:00","type":"turn","payload":{"turn_id":"turn-a","activity_id":"activity-a","model":"model-1","upstream_model":"model-upstream"}}),
            json!({"seq":3,"time":"2026-11-17T12:00:02.000+08:00","type":"turn","payload":{"turn_id":"turn-b","activity_id":"activity-b","model":"model-1","upstream_model":"model-upstream"}}),
            json!({"seq":4,"time":"2026-11-17T12:00:03.000+08:00","type":"turn_end","payload":{"status":"completed","turn_id":"turn-a"}}),
        ],
    );
    let store = RolloutStore::new(root.path().to_path_buf());
    let closed = store
        .find_activity_turn("identity-1", "thread-interleaved", "activity-a")
        .expect("find closed turn")
        .expect("closed turn exists");
    let open = store
        .find_activity_turn("identity-1", "thread-interleaved", "activity-b")
        .expect("find open turn")
        .expect("open turn exists");

    assert!(
        closed.complete,
        "the turn_end carrying turn-a closes activity-a"
    );
    assert_eq!(closed.status, RolloutStatus::Completed);
    assert!(
        !open.complete,
        "activity-b must not be closed by turn-a's turn_end"
    );
}

#[tokio::test]
async fn an_unreadable_turn_end_from_another_turn_does_not_break_the_lookup() {
    let root = tempfile::tempdir().expect("temp rollout root");
    // a 的 turn_end 状态无法解析；查询 b 时不能因为别人那条坏记录失败。
    write_thread_file(
        root.path(),
        "thread-broken-end",
        &[
            json!({"v":1,"type":"meta","thread_id":"thread-broken-end","created_at":"2026-11-17T12:00:00.000+08:00"}),
            json!({"seq":1,"time":"2026-11-17T12:00:00.100+08:00","type":"window_start","payload":{"generation":0}}),
            json!({"seq":2,"time":"2026-11-17T12:00:01.000+08:00","type":"turn","payload":{"turn_id":"turn-a","activity_id":"activity-a","model":"model-1","upstream_model":"model-upstream"}}),
            json!({"seq":3,"time":"2026-11-17T12:00:02.000+08:00","type":"turn","payload":{"turn_id":"turn-b","activity_id":"activity-b","model":"model-1","upstream_model":"model-upstream"}}),
            json!({"seq":4,"time":"2026-11-17T12:00:03.000+08:00","type":"turn_end","payload":{"status":"not-a-status","turn_id":"turn-a"}}),
            json!({"seq":5,"time":"2026-11-17T12:00:04.000+08:00","type":"turn_end","payload":{"status":"completed","turn_id":"turn-b"}}),
        ],
    );
    let store = RolloutStore::new(root.path().to_path_buf());
    let found = store
        .find_activity_turn("identity-1", "thread-broken-end", "activity-b")
        .expect("an unrelated broken turn_end must not fail the lookup")
        .expect("turn exists");

    assert!(found.complete);
    assert_eq!(found.status, RolloutStatus::Completed);
}

mod turn_end;
