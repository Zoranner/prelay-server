use std::collections::BTreeMap;

use chrono::{Datelike, FixedOffset, NaiveDate, Utc};
use sea_orm::{sea_query::Value, ConnectionTrait, Database, DbBackend, MockDatabase};

use crate::{
    schema::initialize,
    stats::{ActivityInsert, ModelStatsScope, StatsRange},
    storage::{MasterKey, Storage, StorageError},
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

    let catalog = crate::test_support::fixture_catalog();
    let rows = storage
        .model_stats_with_catalog(
            &identity,
            ModelStatsScope::Personal,
            StatsRange::All,
            &catalog,
        )
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
async fn model_stats_scope_selects_personal_or_team_aggregation() {
    let storage = test_storage().await;
    let identity_a = register_identity(&storage, "scope-a").await;
    let identity_b = register_identity(&storage, "scope-b").await;
    for (identity, id, model, input_tokens, output_tokens) in [
        (&identity_a, "scope-a-shared", "shared-model", 3, 4),
        (&identity_b, "scope-b-shared", "shared-model", 5, 6),
        (&identity_b, "scope-b-other", "other-model", 7, 8),
    ] {
        let mut log = test_log(Some(input_tokens), Some(output_tokens));
        log.model_requested = model.to_string();
        storage
            .insert_activity_with_id(identity, id.to_string(), log)
            .await
            .expect("insert scoped model log");
    }

    let catalog = crate::test_support::fixture_catalog();
    let personal = storage
        .model_stats_with_catalog(
            &identity_a,
            ModelStatsScope::Personal,
            StatsRange::All,
            &catalog,
        )
        .await
        .expect("load personal model stats");
    assert_eq!(personal.len(), 1);
    assert_eq!(personal[0].model_requested.as_deref(), Some("shared-model"));
    assert_eq!(personal[0].total_requests, 1);
    assert_eq!(personal[0].input_tokens, 3);
    assert_eq!(personal[0].output_tokens, 4);

    let team = storage
        .model_stats_with_catalog(
            &identity_a,
            ModelStatsScope::Team,
            StatsRange::All,
            &catalog,
        )
        .await
        .expect("load team model stats");
    assert_eq!(team.len(), 2);
    assert_eq!(team[0].model_requested.as_deref(), Some("shared-model"));
    assert_eq!(team[0].total_requests, 2);
    assert_eq!(team[0].input_tokens, 8);
    assert_eq!(team[0].output_tokens, 10);
    assert_eq!(team[1].model_requested.as_deref(), Some("other-model"));
    assert_eq!(team[1].total_requests, 1);
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
    assert!(week.iter().any(|point| point.bucket.ends_with(" 06:00:00")));
    assert_eq!(year.len(), 24);
}

#[tokio::test]
async fn daily_timeline_covers_last_year_with_beijing_day_buckets() {
    let storage = test_storage().await;
    let identity = register_identity(&storage, "daily-timeline").await;
    let last_year = Utc::now().with_timezone(&beijing_offset()).year() - 1;
    let activity_id = "daily-boundary";
    storage
        .insert_activity_with_id(
            &identity,
            activity_id.to_string(),
            test_log(Some(3), Some(4)),
        )
        .await
        .expect("insert daily timeline log");
    // UTC 16:30 在东八区已经是次日 00:30：用于验证日界按北京时间切分。
    storage
        .db
        .execute_unprepared(&format!(
            "UPDATE identity_activities SET created_at = '{last_year}-06-15T16:30:00+00:00' WHERE id = '{activity_id}'"
        ))
        .await
        .expect("move activity onto the Beijing day boundary");

    let points = storage
        .daily_token_usage_timeline(&identity, StatsRange::LastYear)
        .await
        .expect("load daily timeline");

    assert_eq!(points.len(), days_in_year(last_year));
    assert_eq!(points[0].bucket, format!("{last_year}-01-01"));
    assert_eq!(
        points.last().expect("last daily bucket").bucket,
        format!("{last_year}-12-31")
    );
    assert!(points.iter().all(|point| is_day_label(&point.bucket)));
    assert!(points
        .windows(2)
        .all(|pair| pair[0].bucket < pair[1].bucket));
    let active = points
        .iter()
        .find(|point| point.bucket == format!("{last_year}-06-16"))
        .expect("Beijing day bucket");
    assert_eq!(active.input_tokens, 3);
    assert_eq!(active.output_tokens, 4);
    assert_eq!(
        points.iter().map(|point| point.input_tokens).sum::<i64>(),
        3
    );
    assert_eq!(
        points.iter().filter(|point| point.input_tokens > 0).count(),
        1
    );
}

#[tokio::test]
async fn daily_timeline_rejects_ranges_wider_than_the_bucket_limit() {
    let storage = test_storage().await;
    let identity = register_identity(&storage, "daily-cap").await;
    storage
        .insert_activity_with_id(
            &identity,
            "daily-cap-log".to_string(),
            test_log(Some(1), Some(1)),
        )
        .await
        .expect("insert capped timeline log");
    storage
        .db
        .execute_unprepared(
            "UPDATE identity_activities SET created_at = '2020-01-01T00:00:00+00:00'",
        )
        .await
        .expect("move activity beyond the daily bucket limit");

    let error = storage
        .daily_token_usage_timeline(&identity, StatsRange::All)
        .await
        .expect_err("range=all beyond the cap must be rejected");

    assert!(
        matches!(error, StorageError::ValidationFailed(_)),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn daily_timeline_uses_a_single_grouped_query() {
    let mock = MockDatabase::new(DbBackend::Sqlite)
        .append_query_results(vec![Vec::<BTreeMap<String, Value>>::new()])
        .into_connection();
    let storage = Storage::from_connection(mock.clone(), MasterKey::from_bytes([0; 32]));
    let this_year = Utc::now().with_timezone(&beijing_offset()).year();

    let points = storage
        .daily_token_usage_timeline("identity-daily", StatsRange::ThisYear)
        .await
        .expect("load daily timeline");
    assert_eq!(points.len(), days_in_year(this_year));
    assert!(points.iter().all(|point| point.input_tokens == 0));

    let log = mock.into_transaction_log();
    let statements = log
        .iter()
        .flat_map(|transaction| transaction.statements())
        .collect::<Vec<_>>();
    assert_eq!(statements.len(), 1, "按天时间线必须只查一次库");
    let sql = &statements[0].sql;
    assert!(sql.contains("GROUP BY"), "{sql}");
    assert!(sql.contains("strftime('%Y-%m-%d'"), "{sql}");
}

#[tokio::test]
async fn daily_timeline_groups_by_beijing_day_on_postgres() {
    let mock = MockDatabase::new(DbBackend::Postgres)
        .append_query_results(vec![Vec::<BTreeMap<String, Value>>::new()])
        .into_connection();
    let storage = Storage::from_connection(mock.clone(), MasterKey::from_bytes([0; 32]));

    storage
        .daily_token_usage_timeline("identity-daily", StatsRange::Today)
        .await
        .expect("load daily timeline");

    let log = mock.into_transaction_log();
    let statements = log
        .iter()
        .flat_map(|transaction| transaction.statements())
        .collect::<Vec<_>>();
    assert_eq!(statements.len(), 1);
    let sql = &statements[0].sql;
    assert!(sql.contains("AT TIME ZONE 'UTC'"), "{sql}");
    assert!(sql.contains("interval '8 hours'"), "{sql}");
    assert!(sql.contains("GROUP BY"), "{sql}");
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

fn beijing_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 60 * 60).expect("valid Beijing offset")
}

fn days_in_year(year: i32) -> usize {
    let start = NaiveDate::from_ymd_opt(year, 1, 1).expect("valid year start");
    let end = NaiveDate::from_ymd_opt(year + 1, 1, 1).expect("valid next year start");
    (end - start).num_days() as usize
}

fn is_day_label(label: &str) -> bool {
    label.len() == 10
        && label.as_bytes()[4] == b'-'
        && label.as_bytes()[7] == b'-'
        && label
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}
