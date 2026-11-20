use serde_json::json;

use crate::domain::activity::RolloutStatus;

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
