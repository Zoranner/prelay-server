use serde_json::json;

use crate::domain::activity::{EventPhase, RolloutStatus};
use crate::store::rollout::{FinishedTurnMaterialization, RolloutStore};

use super::{message, session, turn, write_thread_file};

#[test]
fn activity_turn_end_ignores_turn_ends_owned_by_other_turns() {
    let event = |seq: u64, event_type: &str, payload: serde_json::Value| {
        crate::domain::activity::ConversationEvent {
            seq,
            time: "2026-11-17T12:00:00.000+08:00".to_string(),
            event_type: event_type.to_string(),
            payload,
        }
    };
    let events = vec![
        event(
            1,
            "turn",
            json!({"turn_id":"turn-a","activity_id":"activity-a"}),
        ),
        event(
            2,
            "turn",
            json!({"turn_id":"turn-b","activity_id":"activity-b"}),
        ),
        event(
            3,
            "turn_end",
            json!({"status":"completed","turn_id":"turn-a"}),
        ),
    ];

    assert_eq!(
        super::super::activity::activity_turn_end(&events, "activity-a").map(|end| end.status),
        Some(RolloutStatus::Completed)
    );
    assert!(super::super::activity::activity_turn_end(&events, "activity-b").is_none());
}

#[tokio::test]
async fn recovers_an_interleaved_turn_with_the_full_range_summary() {
    let root = tempfile::tempdir().expect("temp rollout root");
    let store = RolloutStore::new(root.path().to_path_buf());
    let session = session("thread-interleaved-resume");
    let earlier = store
        .begin(
            "identity-1",
            &session,
            turn("activity-a"),
            vec![message(EventPhase::Request, "first")],
        )
        .await
        .expect("begin earlier turn");
    drop(earlier);
    let later = store
        .begin(
            "identity-1",
            &session,
            turn("activity-b"),
            vec![message(EventPhase::Request, "second")],
        )
        .await
        .expect("begin later turn");
    drop(later);

    let summary = store
        .finish_activity_turn(
            "identity-1",
            FinishedTurnMaterialization {
                session: session.clone(),
                activity_id: "activity-a".to_string(),
                response_events: Vec::new(),
                status: RolloutStatus::Partial,
                http_status: None,
                error_code: Some("activity_interrupted".to_string()),
                error_message: Some(
                    "process interrupted before activity rollout completion".to_string(),
                ),
                finished_at: "2026-11-17T12:00:03.000+08:00".to_string(),
            },
        )
        .await
        .expect("recover earlier turn");

    let earlier = store
        .find_activity_turn("identity-1", "thread-interleaved-resume", "activity-a")
        .expect("find earlier turn")
        .expect("earlier turn exists");
    assert!(earlier.complete);
    assert_eq!(earlier.status, RolloutStatus::Partial);
    assert_eq!(summary, earlier.summary("activity-a"));
    assert!(summary.event_count > 2);

    let later = store
        .find_activity_turn("identity-1", "thread-interleaved-resume", "activity-b")
        .expect("find later turn")
        .expect("later turn exists");
    assert!(!later.complete);
}

#[tokio::test]
async fn recovers_a_turn_after_legacy_control_lines_with_its_real_line() {
    let root = tempfile::tempdir().expect("temp rollout root");
    write_thread_file(
        root.path(),
        "thread-legacy-resume",
        &[
            json!({"seq":1,"time":"2026-11-17T12:00:00.000+08:00","type":"session_meta","payload":{"thread_id":"thread-legacy-resume"}}),
            json!({"seq":2,"time":"2026-11-17T12:00:00.500+08:00","type":"media","payload":{"kind":"image"}}),
            json!({"seq":3,"time":"2026-11-17T12:00:01.000+08:00","type":"window_start","payload":{"generation":0}}),
            json!({"seq":4,"time":"2026-11-17T12:00:02.000+08:00","type":"turn","payload":{"turn_id":"turn-c","activity_id":"activity-c","model":"model-1","upstream_model":"model-upstream"}}),
            json!({"seq":5,"time":"2026-11-17T12:00:03.000+08:00","type":"turn","payload":{"turn_id":"turn-a","activity_id":"activity-a","model":"model-1","upstream_model":"model-upstream"}}),
            json!({"seq":6,"time":"2026-11-17T12:00:04.000+08:00","type":"turn","payload":{"turn_id":"turn-b","activity_id":"activity-b","model":"model-1","upstream_model":"model-upstream"}}),
        ],
    );
    let store = RolloutStore::new(root.path().to_path_buf());
    let summary = store
        .finish_activity_turn(
            "identity-1",
            FinishedTurnMaterialization {
                session: session("thread-legacy-resume"),
                activity_id: "activity-a".to_string(),
                response_events: Vec::new(),
                status: RolloutStatus::Partial,
                http_status: None,
                error_code: Some("activity_interrupted".to_string()),
                error_message: Some(
                    "process interrupted before activity rollout completion".to_string(),
                ),
                finished_at: "2026-11-17T12:00:05.000+08:00".to_string(),
            },
        )
        .await
        .expect("recover legacy-control-line turn");

    assert_eq!(summary.start_line, 5);
    assert_eq!(summary.end_line, 7);
    let found = store
        .find_activity_turn("identity-1", "thread-legacy-resume", "activity-a")
        .expect("find recovered turn")
        .expect("recovered turn exists");
    assert_eq!(found.start_line, 5);
    assert_eq!(found.end_line, 7);
}
