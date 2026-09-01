use axum::http::StatusCode;
use prelay_protocol::stats::UserLeaderboardEntry;
use prelay_server::stats::ActivityInsert;

use crate::{auth::register, http::request_json, test_context};

#[tokio::test]
async fn management_leaderboard_returns_all_identity_aggregates_to_any_device() {
    let context = test_context::test_context().await;
    let app = context.app;
    let identity_a = register(&app, "leaderboard-a", "S-1-5-21-100").await;
    let identity_b = register(&app, "leaderboard-b", "S-1-5-21-200").await;
    let identity_a_id = identity_a["identity_id"].as_str().expect("identity A id");
    let identity_b_id = identity_b["identity_id"].as_str().expect("identity B id");
    let credential_a = identity_a["credential"].as_str().expect("credential A");

    for (id, identity_id, status, input_tokens, output_tokens) in [
        ("leaderboard-a-1", identity_a_id, "success", 10, 5),
        ("leaderboard-a-2", identity_a_id, "failed", 20, 10),
        ("leaderboard-b-1", identity_b_id, "success", 40, 20),
    ] {
        context
            .storage
            .insert_activity_with_id(
                identity_id,
                id.to_string(),
                ActivityInsert {
                    status: status.to_string(),
                    input_tokens: Some(input_tokens),
                    output_tokens: Some(output_tokens),
                    ..Default::default()
                },
            )
            .await
            .expect("seed leaderboard activity");
    }

    let (status, leaderboard): (StatusCode, Vec<UserLeaderboardEntry>) = request_json(
        &app,
        "GET",
        "/api/stats/leaderboard?range=today&metric=total_tokens&limit=10",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(leaderboard.len(), 2);
    assert_eq!(leaderboard[0].rank, 1);
    assert_eq!(leaderboard[0].identity_id, identity_b_id);
    assert_eq!(leaderboard[0].activity_count, 1);
    assert_eq!(leaderboard[0].total_tokens, 60);
    assert_eq!(leaderboard[0].successful_activities, 1);
    assert_eq!(leaderboard[0].success_rate, 1.0);
    assert_eq!(leaderboard[1].rank, 2);
    assert_eq!(leaderboard[1].identity_id, identity_a_id);
    assert_eq!(leaderboard[1].activity_count, 2);
    assert_eq!(leaderboard[1].total_tokens, 45);
    assert_eq!(leaderboard[1].successful_activities, 1);
    assert_eq!(leaderboard[1].success_rate, 0.5);

    let leaderboard_json = serde_json::to_value(&leaderboard).expect("serialize leaderboard");
    assert!(leaderboard_json[0].get("machine_id").is_none());
    assert!(leaderboard_json[0].get("account_sid").is_none());
    assert!(leaderboard_json[0].get("credential").is_none());
    assert!(leaderboard_json[0].get("error_message").is_none());
}
