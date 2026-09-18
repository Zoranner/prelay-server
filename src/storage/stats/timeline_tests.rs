use std::collections::BTreeMap;

use chrono::{Datelike, FixedOffset, NaiveDate, Utc};
use sea_orm::{sea_query::Value, ConnectionTrait, DbBackend, MockDatabase};

use crate::{
    stats::{ModelStatsScope, StatsRange},
    storage::{MasterKey, Storage, StorageError},
};

use super::tests::{register_identity, test_log, test_storage};

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
        .token_usage_timeline(&identity, ModelStatsScope::Personal, StatsRange::Today)
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
        .token_usage_timeline(&identity, ModelStatsScope::Personal, StatsRange::ThisWeek)
        .await
        .expect("load week timeline");
    let year = storage
        .token_usage_timeline(&identity, ModelStatsScope::Personal, StatsRange::ThisYear)
        .await
        .expect("load year timeline");

    assert_eq!(week.len(), 28);
    assert!(week.iter().any(|point| point.bucket.ends_with(" 06:00:00")));
    assert_eq!(year.len(), 24);
}

#[tokio::test]
async fn mapped_timelines_merge_bucket_rows_for_every_granularity() {
    let storage = test_storage().await;
    let identity = register_identity(&storage, "merged-timeline").await;
    storage
        .insert_activity_with_id(
            &identity,
            "merged-timeline-log".to_string(),
            test_log(Some(3), Some(4)),
        )
        .await
        .expect("insert merged timeline log");

    for range in [
        StatsRange::Today,
        StatsRange::ThisWeek,
        StatsRange::ThisMonth,
        StatsRange::ThisYear,
        StatsRange::All,
    ] {
        let points = storage
            .token_usage_timeline(&identity, ModelStatsScope::Personal, range)
            .await
            .expect("load merged timeline");
        assert_eq!(
            points.iter().map(|point| point.input_tokens).sum::<i64>(),
            3,
            "{range:?}"
        );
        assert_eq!(
            points.iter().map(|point| point.output_tokens).sum::<i64>(),
            4,
            "{range:?}"
        );
        assert_eq!(
            points.iter().filter(|point| point.input_tokens > 0).count(),
            1,
            "{range:?}"
        );
    }
}

#[tokio::test]
async fn timeline_scope_selects_personal_or_team_aggregation() {
    let storage = test_storage().await;
    let identity_a = register_identity(&storage, "timeline-scope-a").await;
    let identity_b = register_identity(&storage, "timeline-scope-b").await;
    storage
        .insert_activity_with_id(
            &identity_a,
            "timeline-scope-a-log".to_string(),
            test_log(Some(3), Some(4)),
        )
        .await
        .expect("insert identity A log");
    storage
        .insert_activity_with_id(
            &identity_b,
            "timeline-scope-b-log".to_string(),
            test_log(Some(5), Some(6)),
        )
        .await
        .expect("insert identity B log");

    let personal = storage
        .token_usage_timeline(&identity_a, ModelStatsScope::Personal, StatsRange::Today)
        .await
        .expect("load personal timeline");
    assert_eq!(
        personal.iter().map(|point| point.input_tokens).sum::<i64>(),
        3
    );

    let team = storage
        .token_usage_timeline(&identity_a, ModelStatsScope::Team, StatsRange::Today)
        .await
        .expect("load team timeline");
    assert_eq!(team.len(), 24);
    assert_eq!(team.iter().map(|point| point.input_tokens).sum::<i64>(), 8);
    assert_eq!(
        team.iter().map(|point| point.output_tokens).sum::<i64>(),
        10
    );

    let team_daily = storage
        .daily_token_usage_timeline(&identity_a, ModelStatsScope::Team, StatsRange::Today)
        .await
        .expect("load team daily timeline");
    assert_eq!(team_daily.len(), 1);
    assert_eq!(team_daily[0].input_tokens, 8);

    let team_all = storage
        .token_usage_timeline(&identity_a, ModelStatsScope::Team, StatsRange::All)
        .await
        .expect("load team timeline for all ranges");
    assert_eq!(
        team_all.iter().map(|point| point.input_tokens).sum::<i64>(),
        8
    );
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
        .daily_token_usage_timeline(&identity, ModelStatsScope::Personal, StatsRange::LastYear)
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
        .daily_token_usage_timeline(&identity, ModelStatsScope::Personal, StatsRange::All)
        .await
        .expect_err("range=all beyond the cap must be rejected");

    assert!(
        matches!(error, StorageError::ValidationFailed(_)),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn daily_timeline_uses_a_single_grouped_query() {
    let mock = MockDatabase::new(DbBackend::Postgres)
        .append_query_results(vec![Vec::<BTreeMap<String, Value>>::new()])
        .into_connection();
    let storage = Storage::from_connection(mock.clone(), MasterKey::from_bytes([0; 32]));
    let this_year = Utc::now().with_timezone(&beijing_offset()).year();

    let points = storage
        .daily_token_usage_timeline(
            "identity-daily",
            ModelStatsScope::Personal,
            StatsRange::ThisYear,
        )
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
    assert!(sql.contains("'YYYY-MM-DD 00:00:00'"), "{sql}");
    assert!(sql.contains("AT TIME ZONE 'UTC'"), "{sql}");
}

#[tokio::test]
async fn mapped_timelines_use_one_grouped_query_per_request() {
    let mock = MockDatabase::new(DbBackend::Postgres)
        .append_query_results(vec![
            Vec::<BTreeMap<String, Value>>::new(),
            Vec::<BTreeMap<String, Value>>::new(),
        ])
        .into_connection();
    let storage = Storage::from_connection(mock.clone(), MasterKey::from_bytes([0; 32]));

    storage
        .token_usage_timeline(
            "identity-trend",
            ModelStatsScope::Personal,
            StatsRange::Today,
        )
        .await
        .expect("load hourly timeline");
    storage
        .token_usage_timeline(
            "identity-trend",
            ModelStatsScope::Personal,
            StatsRange::ThisMonth,
        )
        .await
        .expect("load daily timeline");

    let log = mock.into_transaction_log();
    let statements = log
        .iter()
        .flat_map(|transaction| transaction.statements())
        .collect::<Vec<_>>();
    assert_eq!(statements.len(), 2, "两次请求各只查一次库");
    let hourly = &statements[0].sql;
    let daily = &statements[1].sql;
    assert!(
        hourly.contains("GROUP BY") && hourly.contains("'YYYY-MM-DD HH24:00:00'"),
        "{hourly}"
    );
    assert!(
        daily.contains("GROUP BY") && daily.contains("'YYYY-MM-DD 00:00:00'"),
        "{daily}"
    );
}

#[tokio::test]
async fn daily_timeline_groups_by_beijing_day_on_postgres() {
    let mock = MockDatabase::new(DbBackend::Postgres)
        .append_query_results(vec![Vec::<BTreeMap<String, Value>>::new()])
        .into_connection();
    let storage = Storage::from_connection(mock.clone(), MasterKey::from_bytes([0; 32]));

    storage
        .daily_token_usage_timeline(
            "identity-daily",
            ModelStatsScope::Personal,
            StatsRange::Today,
        )
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
