use axum::http::StatusCode;
use prelay_server::stats::ActivityInsert;

use crate::{auth::register, http::request_json, test_context};

#[tokio::test]
async fn management_timeline_supports_daily_granularity() {
    let context = test_context::test_context().await;
    let app = context.app;
    let identity = register(&app, "machine-daily", "S-1-5-21-500").await;
    let credential = identity["credential"].as_str().expect("credential");
    let identity_id = identity["identity_id"].as_str().expect("identity id");
    context
        .storage
        .insert_activity_with_id(
            identity_id,
            "request-daily".to_string(),
            ActivityInsert {
                protocol_in: "chat_completions".to_string(),
                protocol_out: "chat_completions".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                endpoint_name: "Daily endpoint".to_string(),
                provider_id: "provider-daily".to_string(),
                provider_name: "Provider Daily".to_string(),
                model_requested: "deepseek-v4-flash".to_string(),
                model_upstream: "deepseek-v4-flash".to_string(),
                status: "success".to_string(),
                http_status: 200,
                input_tokens: Some(3),
                output_tokens: Some(4),
                latency_ms: 120,
                ..Default::default()
            },
        )
        .await
        .expect("seed daily activity");

    let (status, points): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=last_year&granularity=day",
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(points.len(), 365 | 366),
        "unexpected bucket count: {}",
        points.len()
    );
    let first = points[0]["bucket"].as_str().expect("first bucket");
    let last = points[points.len() - 1]["bucket"]
        .as_str()
        .expect("last bucket");
    assert!(first.ends_with("-01-01"), "{first}");
    assert!(last.ends_with("-12-31"), "{last}");
    assert_eq!(first[..4], last[..4], "{first}..{last}");
    assert!(points.iter().all(|point| point["bucket"]
        .as_str()
        .is_some_and(|bucket| bucket.len() == 10)));
    assert!(points.iter().all(|point| point["input_tokens"] == 0));

    let (status, year_points): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=this_year&granularity=day",
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        year_points
            .iter()
            .map(|point| point["input_tokens"].as_i64().expect("input tokens"))
            .sum::<i64>(),
        3
    );
}
