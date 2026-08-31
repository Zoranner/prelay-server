use axum::http::StatusCode;
use prelay_protocol::{
    ActivitySummary, CreateProviderRequest, ModelStatsSummary, ProviderStatsSummary, StatsOverview,
};
use prelay_server::{stats::ActivityInsert, storage::Storage};

use crate::{auth::register, http::request_json, test_context};

struct ActivitySeed<'a> {
    id: &'a str,
    identity_id: &'a str,
    provider_id: &'a str,
    provider_name: &'a str,
    model_requested: &'a str,
    status: &'a str,
    input_tokens: i64,
    output_tokens: i64,
}

async fn seed_activity(storage: &Storage, seed: ActivitySeed<'_>) {
    storage
        .insert_activity_with_id(
            seed.identity_id,
            seed.id.to_string(),
            ActivityInsert {
                protocol_in: "chat_completions".to_string(),
                protocol_out: "chat_completions".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                endpoint_name: "Test endpoint".to_string(),
                provider_id: seed.provider_id.to_string(),
                provider_name: seed.provider_name.to_string(),
                model_requested: seed.model_requested.to_string(),
                model_upstream: seed.model_requested.to_string(),
                status: seed.status.to_string(),
                http_status: 200,
                input_tokens: Some(seed.input_tokens),
                output_tokens: Some(seed.output_tokens),
                cache_read_tokens: Some(1),
                cache_write_tokens: Some(2),
                latency_ms: 120,
                ..Default::default()
            },
        )
        .await
        .expect("seed activity");
}

#[tokio::test]
async fn cache_rate_uses_normalized_total_input_tokens() {
    let context = test_context::test_context().await;
    let app = context.app;
    let identity = register(&app, "cache-machine", "S-1-5-21-300").await;
    let credential = identity["credential"].as_str().expect("credential");
    let identity_id = identity["identity_id"].as_str().expect("identity id");

    for (id, protocol_in, protocol_upstream, input_tokens, cache_read_tokens, cache_write_tokens) in [
        ("openai-cache", "responses", "responses", 3, 2, 0),
        (
            "anthropic-cache",
            "anthropic_messages",
            "anthropic_messages",
            3,
            2,
            1,
        ),
        ("unknown-cache", "unknown", "future_protocol", 5, 2, 1),
    ] {
        context
            .storage
            .insert_activity_with_id(
                identity_id,
                id.to_string(),
                ActivityInsert {
                    protocol_in: protocol_in.to_string(),
                    protocol_out: protocol_in.to_string(),
                    protocol_upstream: protocol_upstream.to_string(),
                    endpoint_name: "Cache endpoint".to_string(),
                    provider_id: String::new(),
                    provider_name: String::new(),
                    model_requested: "cache-model".to_string(),
                    model_upstream: "cache-model".to_string(),
                    status: "success".to_string(),
                    http_status: 200,
                    input_tokens: Some(input_tokens),
                    output_tokens: Some(1),
                    cache_read_tokens: Some(cache_read_tokens),
                    cache_write_tokens: Some(cache_write_tokens),
                    latency_ms: 10,
                    ..Default::default()
                },
            )
            .await
            .expect("seed cache log");
    }

    let (status, overview): (StatusCode, serde_json::Value) = request_json(
        &app,
        "GET",
        "/api/stats/overview?range=today",
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview["cache_read_tokens"], 6);
    assert_eq!(overview["total_input_tokens"], 17);

    let (status, timeline): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=today",
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        timeline
            .iter()
            .map(|point| point["total_input_tokens"]
                .as_i64()
                .expect("total input tokens"))
            .sum::<i64>(),
        17
    );
}

#[tokio::test]
async fn management_stats_only_return_the_current_identity_request_data() {
    let context = test_context::test_context().await;
    let app = context.app;

    let identity_a = register(&app, "machine-a", "S-1-5-21-100").await;
    let identity_a_id = identity_a["identity_id"].as_str().expect("identity A id");
    let credential_a = identity_a["credential"].as_str().expect("credential A");
    let (status, provider_a): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential_a),
        Some(
            serde_json::to_value(CreateProviderRequest {
                name: "Provider A".to_string(),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider-a.example".to_string(),
                api_key: "sk-a".to_string(),
                capabilities: None,
                models: vec!["model-a".to_string()],
            })
            .expect("serialize provider A"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_a_id = provider_a["id"].as_str().expect("provider A id");

    let identity_b = register(&app, "machine-b", "S-1-5-21-200").await;
    let identity_b_id = identity_b["identity_id"].as_str().expect("identity B id");
    let credential_b = identity_b["credential"].as_str().expect("credential B");
    let (status, provider_b): (StatusCode, serde_json::Value) = request_json(
        &app,
        "POST",
        "/api/providers",
        Some(credential_b),
        Some(
            serde_json::to_value(CreateProviderRequest {
                name: "Provider B".to_string(),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider-b.example".to_string(),
                api_key: "sk-b".to_string(),
                capabilities: None,
                models: vec!["model-b".to_string()],
            })
            .expect("serialize provider B"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let provider_b_id = provider_b["id"].as_str().expect("provider B id");

    seed_activity(
        &context.storage,
        ActivitySeed {
            id: "request-a",
            identity_id: identity_a_id,
            provider_id: provider_a_id,
            provider_name: "Provider A",
            model_requested: "model-a",
            status: "success",
            input_tokens: 3,
            output_tokens: 4,
        },
    )
    .await;
    seed_activity(
        &context.storage,
        ActivitySeed {
            id: "request-b",
            identity_id: identity_b_id,
            provider_id: provider_b_id,
            provider_name: "Provider B",
            model_requested: "model-b",
            status: "failed",
            input_tokens: 5,
            output_tokens: 6,
        },
    )
    .await;

    let (status, overview_a): (StatusCode, StatsOverview) = request_json(
        &app,
        "GET",
        "/api/stats/overview?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_a.total_requests, 1);
    assert_eq!(overview_a.successful_requests, 1);
    assert_eq!(overview_a.failed_requests, 0);
    assert_eq!(overview_a.input_tokens, 3);
    assert_eq!(overview_a.output_tokens, 4);
    assert_eq!(overview_a.cache_read_tokens, 1);
    assert_eq!(overview_a.cache_write_tokens, 2);
    assert_eq!(overview_a.average_latency_ms, Some(120));

    let (status, timeline_a): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(timeline_a.len(), 24);
    assert_eq!(
        timeline_a
            .iter()
            .map(|point| point["input_tokens"].as_i64().expect("input tokens"))
            .sum::<i64>(),
        3
    );

    let (status, activities_a): (StatusCode, Vec<ActivitySummary>) = request_json(
        &app,
        "GET",
        "/api/stats/activities",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(activities_a.len(), 1);
    assert_eq!(activities_a[0].id, "request-a");
    assert_eq!(activities_a[0].provider_name.as_deref(), Some("Provider A"));

    let (status, models_a): (StatusCode, Vec<ModelStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/models?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models_a.len(), 1);
    assert_eq!(models_a[0].model_requested.as_deref(), Some("model-a"));
    assert_eq!(models_a[0].total_requests, 1);
    assert_eq!(models_a[0].input_tokens, 3);
    assert_eq!(models_a[0].output_tokens, 4);

    let (status, providers_a): (StatusCode, Vec<ProviderStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/providers?range=today",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers_a.len(), 1);
    assert_eq!(providers_a[0].provider_id.as_deref(), Some(provider_a_id));
    assert_eq!(providers_a[0].provider_name.as_deref(), Some("Provider A"));
    assert_eq!(providers_a[0].total_requests, 1);

    let (status, overview_a_all): (StatusCode, StatsOverview) = request_json(
        &app,
        "GET",
        "/api/stats/overview?range=all",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_a_all.total_requests, 1);
    assert_eq!(overview_a_all.input_tokens, 3);
    assert_eq!(overview_a_all.output_tokens, 4);

    let (status, timeline_a_all): (StatusCode, Vec<serde_json::Value>) = request_json(
        &app,
        "GET",
        "/api/stats/timeline?range=all",
        Some(credential_a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(timeline_a_all.len(), 1);
    assert_eq!(
        timeline_a_all
            .iter()
            .map(|point| point["input_tokens"].as_i64().expect("input tokens"))
            .sum::<i64>(),
        3
    );

    let (status, overview_b): (StatusCode, StatsOverview) =
        request_json(&app, "GET", "/api/stats/overview", Some(credential_b), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview_b.total_requests, 1);
    assert_eq!(overview_b.successful_requests, 0);
    assert_eq!(overview_b.failed_requests, 1);
    assert_eq!(overview_b.input_tokens, 5);
    assert_eq!(overview_b.output_tokens, 6);
    assert_eq!(overview_b.cache_read_tokens, 1);
    assert_eq!(overview_b.cache_write_tokens, 2);
    assert_eq!(overview_b.average_latency_ms, Some(120));

    let (status, activities_b): (StatusCode, Vec<ActivitySummary>) = request_json(
        &app,
        "GET",
        "/api/stats/activities",
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(activities_b.len(), 1);
    assert_eq!(activities_b[0].id, "request-b");
    assert_eq!(activities_b[0].provider_name.as_deref(), Some("Provider B"));

    let (status, models_b): (StatusCode, Vec<ModelStatsSummary>) =
        request_json(&app, "GET", "/api/stats/models", Some(credential_b), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models_b.len(), 1);
    assert_eq!(models_b[0].model_requested.as_deref(), Some("model-b"));
    assert_eq!(models_b[0].total_requests, 1);
    assert_eq!(models_b[0].input_tokens, 5);
    assert_eq!(models_b[0].output_tokens, 6);

    let (status, providers_b): (StatusCode, Vec<ProviderStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/providers",
        Some(credential_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers_b.len(), 1);
    assert_eq!(providers_b[0].provider_id.as_deref(), Some(provider_b_id));
    assert_eq!(providers_b[0].provider_name.as_deref(), Some("Provider B"));
    assert_eq!(providers_b[0].total_requests, 1);
}
