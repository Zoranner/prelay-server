use crate::domain::activity::rollout::{RolloutStatus, RolloutSummary};
use crate::domain::stats::ActivityInsert;
use crate::store::storage::RecoveredRollout;

#[tokio::test]
async fn inserts_and_completes_rollout_index() {
    let storage = crate::test_support::test_storage().await;
    let identity = storage
        .register_identity(
            "rollout-machine",
            "S-rollout",
            &crate::domain::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    let activity_id = "rollout-activity";
    let rollout = RolloutSummary {
        activity_id: activity_id.to_string(),
        thread_id: "thread-1".to_string(),
        window_id: None,
        generation: 0,
        relative_path: "identity/thread-1.jsonl".to_string(),
        status: RolloutStatus::Writing,
        start_line: 1,
        end_line: 2,
        event_count: 2,
        byte_size: 128,
        sha256: "a".repeat(64),
    };
    storage
        .insert_activity_with_rollout(
            &identity.identity_id,
            activity_id.to_string(),
            test_activity(),
            rollout,
        )
        .await
        .expect("insert activity and rollout");

    let found = storage
        .find_activity_rollout(activity_id)
        .await
        .expect("find rollout")
        .expect("rollout exists");
    assert_eq!(found.status, "writing");
    assert_eq!(found.session_id, "thread-1");
    assert_eq!(found.thread_id, "thread-1");

    storage
        .complete_activity_rollout(
            activity_id,
            RolloutSummary {
                activity_id: activity_id.to_string(),
                thread_id: "thread-1".to_string(),
                window_id: None,
                generation: 0,
                relative_path: "identity/thread-1.jsonl".to_string(),
                status: RolloutStatus::Completed,
                start_line: 1,
                end_line: 4,
                event_count: 4,
                byte_size: 256,
                sha256: "b".repeat(64),
            },
        )
        .await
        .expect("complete rollout");
    let found = storage
        .find_activity_rollout(activity_id)
        .await
        .expect("find completed rollout")
        .expect("completed rollout exists");
    assert_eq!(found.status, "completed");
    assert_eq!(found.event_count, 4);
    assert!(found.completed_at.is_some());
}

#[tokio::test]
async fn inserts_a_final_rollout_with_its_completed_status() {
    let storage = crate::test_support::test_storage().await;
    let identity = storage
        .register_identity(
            "rollout-final-machine",
            "S-rollout-final",
            &crate::domain::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    let activity_id = "completed-rollout-activity";
    storage
        .insert_activity_with_rollout(
            &identity.identity_id,
            activity_id.to_string(),
            test_activity(),
            RolloutSummary {
                activity_id: activity_id.to_string(),
                thread_id: "thread-final".to_string(),
                window_id: None,
                generation: 0,
                relative_path: "identity/thread-final.jsonl".to_string(),
                status: RolloutStatus::Completed,
                start_line: 1,
                end_line: 3,
                event_count: 3,
                byte_size: 192,
                sha256: "c".repeat(64),
            },
        )
        .await
        .expect("insert completed rollout");

    let found = storage
        .find_activity_rollout(activity_id)
        .await
        .expect("find completed rollout")
        .expect("completed rollout exists");
    assert_eq!(found.status, "completed");
    assert!(found.completed_at.is_some());
}

#[tokio::test]
async fn recovers_interrupted_activity_and_rollout_status_together() {
    let storage = crate::test_support::test_storage().await;
    let identity = storage
        .register_identity(
            "recover-machine",
            "S-recover",
            &crate::domain::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    let activity_id = "interrupted-activity";
    let mut activity = test_activity();
    activity.status = "streaming".to_string();
    activity.is_streaming = true;
    storage
        .insert_activity_with_rollout(
            &identity.identity_id,
            activity_id.to_string(),
            activity,
            RolloutSummary {
                activity_id: activity_id.to_string(),
                thread_id: "thread-interrupted".to_string(),
                window_id: None,
                generation: 0,
                relative_path: "identity/thread-interrupted.jsonl".to_string(),
                status: RolloutStatus::Writing,
                start_line: 1,
                end_line: 2,
                event_count: 2,
                byte_size: 128,
                sha256: "d".repeat(64),
            },
        )
        .await
        .expect("insert interrupted activity");

    assert_eq!(
        storage
            .recover_interrupted_activities()
            .await
            .expect("recover interrupted activity"),
        1
    );

    let activity = storage
        .list_activities(&identity.identity_id, 1)
        .await
        .expect("load recovered activity")
        .pop()
        .expect("recovered activity");
    assert_eq!(activity.status, "failed");
    assert_eq!(activity.error_code.as_deref(), Some("activity_interrupted"));
    let rollout = storage
        .find_activity_rollout(activity_id)
        .await
        .expect("load recovered rollout")
        .expect("recovered rollout");
    assert_eq!(rollout.status, "partial");
    assert!(rollout.last_error.is_some());
}

#[tokio::test]
async fn applies_a_verified_recovered_rollout_to_both_tables() {
    let storage = crate::test_support::test_storage().await;
    let identity = storage
        .register_identity(
            "recover-completed-machine",
            "S-recover-completed",
            &crate::domain::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    let activity_id = "recover-completed-activity";
    let mut activity = test_activity();
    activity.status = "streaming".to_string();
    activity.is_streaming = true;
    storage
        .insert_activity_with_rollout(
            &identity.identity_id,
            activity_id.to_string(),
            activity,
            RolloutSummary {
                activity_id: activity_id.to_string(),
                thread_id: "thread-recovered".to_string(),
                window_id: None,
                generation: 0,
                relative_path: "identity/thread-recovered.jsonl".to_string(),
                status: RolloutStatus::Writing,
                start_line: 0,
                end_line: 0,
                event_count: 0,
                byte_size: 0,
                sha256: String::new(),
            },
        )
        .await
        .expect("insert writing activity");

    storage
        .recover_rollout_status(RecoveredRollout {
            activity_id: activity_id.to_string(),
            status: RolloutStatus::Completed,
            http_status: Some(200),
            error_code: None,
            error_message: None,
            event_count: Some(4),
            byte_size: Some(512),
            sha256: Some("e".repeat(64)),
        })
        .await
        .expect("recover completed rollout");

    let activity = storage
        .list_activities(&identity.identity_id, 1)
        .await
        .expect("load recovered activity")
        .pop()
        .expect("recovered activity");
    assert_eq!(activity.status, "success");
    assert_eq!(activity.http_status, Some(200));
    let rollout = storage
        .find_activity_rollout(activity_id)
        .await
        .expect("load recovered rollout")
        .expect("recovered rollout");
    assert_eq!(rollout.status, "completed");
    assert_eq!(rollout.event_count, 4);
    assert_eq!(rollout.byte_size, 512);
    assert!(rollout.completed_at.is_some());
}

fn test_activity() -> ActivityInsert {
    ActivityInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: "responses".to_string(),
        endpoint_name: "Codex".to_string(),
        provider_id: "provider-1".to_string(),
        provider_name: "DeepSeek".to_string(),
        provider_type: "deepseek".to_string(),
        model_requested: "deepseek-flash".to_string(),
        model_upstream: "deepseek-chat".to_string(),
        status: "success".to_string(),
        http_status: 200,
        error_code: None,
        error_message: None,
        is_streaming: false,
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms: 1,
        upstream_latency_ms: None,
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: None,
    }
}
