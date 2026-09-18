use crate::stats::StatsRange;

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
