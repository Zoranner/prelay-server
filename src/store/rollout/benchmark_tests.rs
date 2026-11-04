use std::time::Instant;

use serde_json::json;

use super::RolloutStore;
use crate::domain::activity::rollout::{RolloutStatus, SessionContext, TurnContext};
use crate::domain::activity::{ContentDraft, ContentEventBody, ConversationMessage, EventPhase};

const HISTORY_EVENTS: usize = 20_000;

fn test_meta() -> TurnContext {
    TurnContext {
        activity_id: "activity-benchmark".to_string(),
        model_requested: "deepseek-flash".to_string(),
        model_upstream: "deepseek/deepseek-v4.1-flash".to_string(),
    }
}

fn history() -> Vec<ContentDraft> {
    (0..HISTORY_EVENTS)
        .map(|index| {
            ContentDraft::new(ContentEventBody::Message(ConversationMessage {
                phase: EventPhase::Request,
                role: "user".to_string(),
                message_phase: None,
                content: json!(format!("history-{index}")),
            }))
        })
        .collect()
}

#[tokio::test]
#[ignore = "run manually with --ignored to benchmark long-thread cache behavior"]
async fn benchmark_long_thread_cache_hit() {
    let root = tempfile::tempdir().expect("temporary rollout root");
    let session = SessionContext::new("thread-benchmark");
    let history = history();

    let seed = RolloutStore::new(root.path().to_path_buf());
    let mut seed_writer = seed
        .begin("identity-1", &session, test_meta(), history.clone())
        .await
        .expect("write benchmark history");
    seed_writer
        .finish(RolloutStatus::Completed)
        .expect("finish benchmark history");
    std::fs::remove_file(root.path().join("identity-1/thread-benchmark.jsonl.index"))
        .expect("remove benchmark index");

    let store = RolloutStore::new(root.path().to_path_buf());
    let cold_started = Instant::now();
    let mut cold = store
        .begin("identity-1", &session, test_meta(), history.clone())
        .await
        .expect("cold load benchmark history");
    cold.finish(RolloutStatus::Completed)
        .expect("finish cold load");
    let cold_elapsed = cold_started.elapsed();

    let indexed_store = RolloutStore::new(root.path().to_path_buf());
    let indexed_started = Instant::now();
    let mut indexed = indexed_store
        .begin("identity-1", &session, test_meta(), history.clone())
        .await
        .expect("load benchmark history from index");
    indexed
        .finish(RolloutStatus::Completed)
        .expect("finish indexed load");
    let indexed_elapsed = indexed_started.elapsed();

    let mut warm_history = history;
    warm_history.push(ContentDraft::new(ContentEventBody::Message(
        ConversationMessage {
            phase: EventPhase::Request,
            role: "user".to_string(),
            message_phase: None,
            content: json!("new tail"),
        },
    )));
    let warm_started = Instant::now();
    let mut warm = store
        .begin("identity-1", &session, test_meta(), warm_history)
        .await
        .expect("warm load benchmark history");
    warm.finish(RolloutStatus::Completed)
        .expect("finish warm load");
    let warm_elapsed = warm_started.elapsed();

    println!(
        "history_events={HISTORY_EVENTS} cold={cold_elapsed:?} indexed={indexed_elapsed:?} warm={warm_elapsed:?} file_bytes={}",
        std::fs::metadata(cold.final_path())
            .expect("benchmark file metadata")
            .len()
    );
}
