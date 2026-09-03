use sea_orm::Database;

use crate::{
    provider_catalog::ProviderCatalog,
    schema::initialize,
    stats::{ActivityInsert, StatsRange},
    storage::{MasterKey, Storage},
};

#[tokio::test]
async fn model_stats_resolve_display_names_without_splitting_model_ids() {
    let storage = test_storage().await;
    let identity = register_identity(&storage, "model-display-names").await;
    for id in ["model-known-1", "model-known-2"] {
        let mut log = test_log(Some(3), Some(4));
        log.model_requested = "gpt-5.6-luna".to_string();
        storage
            .insert_activity_with_id(&identity, id.to_string(), log)
            .await
            .expect("insert known model log");
    }
    let mut unknown = test_log(Some(5), Some(6));
    unknown.model_requested = "unknown-model".to_string();
    storage
        .insert_activity_with_id(&identity, "model-unknown".to_string(), unknown)
        .await
        .expect("insert unknown model log");

    let catalog = ProviderCatalog::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("config/catalog")
            .as_path(),
    )
    .expect("load provider catalog");
    let rows = storage
        .model_stats_with_catalog(&identity, StatsRange::All, &catalog)
        .await
        .expect("load model stats with catalog");

    let known = rows
        .iter()
        .find(|row| row.model_requested.as_deref() == Some("gpt-5.6-luna"))
        .expect("known model stats");
    assert_eq!(
        known.model_requested_display_name.as_deref(),
        Some("GPT-5.6 Luna")
    );
    assert_eq!(known.total_requests, 2);
    let unknown = rows
        .iter()
        .find(|row| row.model_requested.as_deref() == Some("unknown-model"))
        .expect("unknown model stats");
    assert_eq!(
        unknown.model_requested_display_name.as_deref(),
        Some("unknown-model")
    );
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn overview_is_scoped_to_one_identity() {
    let storage = test_storage().await;
    let identity_a = register_identity(&storage, "stats-a").await;
    let identity_b = register_identity(&storage, "stats-b").await;
    storage
        .insert_activity_with_id(
            &identity_a,
            "stats-a-success".to_string(),
            test_log(Some(3), Some(4)),
        )
        .await
        .expect("insert identity A log");
    let mut failed = test_log(Some(50), Some(60));
    failed.status = "failed".to_string();
    failed.http_status = 502;
    storage
        .insert_activity_with_id(&identity_b, "stats-b-failed".to_string(), failed)
        .await
        .expect("insert identity B log");

    let overview = storage
        .stats_overview(&identity_a, StatsRange::Today)
        .await
        .expect("load identity A overview");

    assert_eq!(overview.total_requests, 1);
    assert_eq!(overview.successful_requests, 1);
    assert_eq!(overview.failed_requests, 0);
    assert_eq!(overview.input_tokens, 3);
    assert_eq!(overview.output_tokens, 4);
}

#[tokio::test]
async fn today_timeline_fills_empty_beijing_hour_buckets() {
    let storage = test_storage().await;
    let identity = register_identity(&storage, "timeline").await;
    storage
        .insert_activity_with_id(
            &identity,
            "timeline-log".to_string(),
            test_log(Some(3), Some(4)),
        )
        .await
        .expect("insert timeline log");

    let timeline = storage
        .token_usage_timeline(&identity, StatsRange::Today)
        .await
        .expect("load today timeline");

    assert_eq!(timeline.len(), 24);
    assert_eq!(
        timeline.iter().map(|point| point.input_tokens).sum::<i64>(),
        3
    );
    assert!(
        timeline
            .iter()
            .filter(|point| point.input_tokens == 0)
            .count()
            >= 23
    );
    assert!(timeline
        .windows(2)
        .all(|pair| pair[0].bucket < pair[1].bucket));
}

#[tokio::test]
async fn week_and_year_timelines_use_dense_buckets() {
    let storage = test_storage().await;
    let identity = register_identity(&storage, "dense-timeline").await;

    let week = storage
        .token_usage_timeline(&identity, StatsRange::ThisWeek)
        .await
        .expect("load week timeline");
    let year = storage
        .token_usage_timeline(&identity, StatsRange::ThisYear)
        .await
        .expect("load year timeline");

    assert_eq!(week.len(), 28);
    assert_eq!(year.len(), 24);
}

async fn test_storage() -> Storage {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect test database");
    initialize(&db).await.expect("initialize test schema");
    Storage::from_connection(db, MasterKey::from_bytes([0; 32]))
}

async fn register_identity(storage: &Storage, suffix: &str) -> String {
    storage
        .register_identity(
            &format!("machine-{suffix}"),
            &format!("sid-{suffix}"),
            &crate::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity")
        .identity_id
}

fn test_log(input_tokens: Option<i64>, output_tokens: Option<i64>) -> ActivityInsert {
    ActivityInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: "chat_completions".to_string(),
        endpoint_name: String::new(),
        provider_id: "provider-1".to_string(),
        provider_name: "Provider One".to_string(),
        model_requested: "model-1".to_string(),
        model_upstream: "model-1".to_string(),
        status: "success".to_string(),
        http_status: 200,
        error_code: None,
        error_message: None,
        is_streaming: false,
        input_tokens,
        output_tokens,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms: 10,
        upstream_latency_ms: None,
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: None,
    }
}
