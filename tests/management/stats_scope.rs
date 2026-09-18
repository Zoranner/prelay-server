use axum::http::StatusCode;
use prelay_server::stats::ActivityInsert;

use crate::{auth::register, http::request_json, test_context};

fn input_token_sum(points: &[serde_json::Value]) -> i64 {
    points
        .iter()
        .map(|point| point["input_tokens"].as_i64().expect("input tokens"))
        .sum()
}

#[tokio::test]
async fn management_stats_scope_switches_between_personal_and_team() {
    let context = test_context::test_context().await;
    let app = context.app;
    let identity_a = register(&app, "scope-machine-a", "S-1-5-21-600").await;
    let identity_b = register(&app, "scope-machine-b", "S-1-5-21-700").await;
    let credential_a = identity_a["credential"].as_str().expect("credential A");
    let identity_a_id = identity_a["identity_id"].as_str().expect("identity A id");
    let identity_b_id = identity_b["identity_id"].as_str().expect("identity B id");

    for (id, identity_id, provider_name, input_tokens, output_tokens) in [
        ("scope-a-log", identity_a_id, "Provider Scope A", 3, 4),
        ("scope-b-log", identity_b_id, "Provider Scope B", 5, 6),
    ] {
        context
            .storage
            .insert_activity_with_id(
                identity_id,
                id.to_string(),
                ActivityInsert {
                    protocol_in: "chat_completions".to_string(),
                    protocol_out: "chat_completions".to_string(),
                    protocol_upstream: "chat_completions".to_string(),
                    endpoint_name: "Scope endpoint".to_string(),
                    provider_id: provider_name.to_string(),
                    provider_name: provider_name.to_string(),
                    model_requested: "scope-model".to_string(),
                    model_upstream: "scope-model".to_string(),
                    status: "success".to_string(),
                    http_status: 200,
                    input_tokens: Some(input_tokens),
                    output_tokens: Some(output_tokens),
                    latency_ms: 20,
                    ..Default::default()
                },
            )
            .await
            .expect("seed scope activity");
    }

    let (status, overview_personal): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        "/api/stats/overview?range=today&scope=personal",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, overview_default): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        "/api/stats/overview?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_default, overview_personal);
    assert_eq!(overview_default["total_requests"], 1);
    assert_eq!(overview_default["input_tokens"], 3);

    let (status, overview_team): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        "/api/stats/overview?range=today&scope=team",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_team["total_requests"], 2);
    assert_eq!(overview_team["successful_requests"], 2);
    assert_eq!(overview_team["input_tokens"], 8);
    assert_eq!(overview_team["output_tokens"], 10);

    let (status, timeline_default): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(input_token_sum(&timeline_default), 3);

    let (status, timeline_team): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=today&scope=team",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(input_token_sum(&timeline_team), 8);

    let (status, timeline_team_daily): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=today&scope=team&granularity=day",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(timeline_team_daily.len(), 1);
    assert_eq!(input_token_sum(&timeline_team_daily), 8);

    let (status, providers_default): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/providers?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers_default.len(), 1);
    assert_eq!(providers_default[0]["provider_name"], "Provider Scope A");

    let (status, providers_team): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/providers?range=today&scope=team",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers_team.len(), 2);
    assert!(providers_team
        .iter()
        .any(|row| row["provider_name"] == "Provider Scope A" && row["input_tokens"] == 3));
    assert!(providers_team
        .iter()
        .any(|row| row["provider_name"] == "Provider Scope B" && row["input_tokens"] == 5));
}
