use chrono::{DateTime, Utc};
use sea_orm::{
    sea_query::Expr, ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter,
    QuerySelect,
};

use crate::{
    entity::identity::activities as identity_activities,
    stats::{
        all_daily_bounds, all_timeline_bounds, parse_beijing_timestamp, timeline_buckets,
        StatsRange, TimeBounds, TimelineBucket, TimelineGranularity, TokenUsageTimelinePoint,
    },
    storage::{Storage, StorageError},
};

use super::{aggregate_query, integer_sum, total_input_tokens_expr};

/// 按天粒度一次最多返回的自然日数量：覆盖一整年（366 天）并留出余量，
/// 更宽的窗口（例如 range=all 的历史超过 400 天）按非法请求拒绝。
const MAX_DAILY_BUCKETS: usize = 400;

impl Storage {
    /// 按范围映射的粒度：一次分组查询取回有数据的桶，再在内存里合并到目标粒度。
    pub async fn token_usage_timeline(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
        timeline(&self.db, identity_id, range, range.timeline_granularity()).await
    }

    /// 强制按北京时间自然日聚合的时间线；同样只查一次库。
    pub async fn daily_token_usage_timeline(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
        timeline(&self.db, identity_id, range, TimelineGranularity::Day).await
    }
}

async fn timeline(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
    granularity: TimelineGranularity,
) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
    let now = Utc::now();
    let bounds = match range.bounds(now) {
        Some(bounds) => bounds,
        None => {
            let Some(earliest) = earliest_log_time(db, identity_id).await? else {
                return Ok(Vec::new());
            };
            match granularity {
                TimelineGranularity::Day => all_daily_bounds(earliest, now),
                _ => all_timeline_bounds(earliest, now),
            }
        }
    };
    let buckets = timeline_buckets(bounds, granularity);
    if matches!(granularity, TimelineGranularity::Day) && buckets.len() > MAX_DAILY_BUCKETS {
        return Err(StorageError::ValidationFailed(format!(
            "granularity=day supports at most {MAX_DAILY_BUCKETS} buckets, requested {}",
            buckets.len()
        )));
    }
    let rows = bucket_totals(db, identity_id, bounds, granularity).await?;
    merge_buckets(buckets, rows)
}

/// SQL 只按小时或按天分组，更粗的粒度在内存里合并，避免每个桶查一次库。
async fn bucket_totals(
    db: &DatabaseConnection,
    identity_id: &str,
    bounds: TimeBounds,
    granularity: TimelineGranularity,
) -> Result<Vec<BucketTokenAggregate>, StorageError> {
    let bucket = beijing_bucket_expr(query_bucket(granularity));
    Ok(aggregate_query(identity_id, Some(bounds))
        .select_only()
        .expr_as(bucket.clone(), "bucket")
        .column_as(
            integer_sum(identity_activities::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(total_input_tokens_expr(), "total_input_tokens")
        .column_as(
            integer_sum(identity_activities::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(
            integer_sum(identity_activities::Column::CacheReadTokens.sum()),
            "cache_read_tokens",
        )
        .column_as(
            integer_sum(identity_activities::Column::CacheWriteTokens.sum()),
            "cache_write_tokens",
        )
        .group_by(bucket)
        .into_model::<BucketTokenAggregate>()
        .all(db)
        .await?)
}

fn merge_buckets(
    buckets: Vec<TimelineBucket>,
    rows: Vec<BucketTokenAggregate>,
) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
    let mut totals = vec![UsageTotals::default(); buckets.len()];
    for row in rows {
        let start = parse_beijing_timestamp(&row.bucket).ok_or_else(|| {
            StorageError::InvalidTimestamp(format!("invalid timeline bucket: {}", row.bucket))
        })?;
        let index = buckets.partition_point(|bucket| bucket.bounds.start <= start);
        let in_range = index
            .checked_sub(1)
            .and_then(|index| buckets.get(index))
            .is_some_and(|bucket| start < bucket.bounds.end);
        if !in_range {
            return Err(StorageError::InvalidTimestamp(format!(
                "timeline bucket {} is outside the requested range",
                row.bucket
            )));
        }
        totals[index - 1].add(row.into_totals());
    }
    Ok(buckets
        .into_iter()
        .zip(totals)
        .map(|(bucket, totals)| totals.into_point(bucket.label))
        .collect())
}

/// 一次查询使用的分组键：小时粒度按小时，其余按天。
#[derive(Clone, Copy)]
enum QueryBucket {
    Hour,
    Day,
}

fn query_bucket(granularity: TimelineGranularity) -> QueryBucket {
    match granularity {
        TimelineGranularity::Hour | TimelineGranularity::SixHours => QueryBucket::Hour,
        _ => QueryBucket::Day,
    }
}

/// 北京时间桶起始时间的分组表达式。created_at 以 RFC3339 UTC 文本保存，
/// 先按 UTC 解析成 timestamptz，再加 8 小时得到北京时间。
fn beijing_bucket_expr(bucket: QueryBucket) -> Expr {
    let format = match bucket {
        QueryBucket::Hour => "YYYY-MM-DD HH24:00:00",
        QueryBucket::Day => "YYYY-MM-DD 00:00:00",
    };
    Expr::cust(format!(
        "to_char((identity_activities.created_at::timestamptz AT TIME ZONE 'UTC') + interval '8 hours', '{format}')"
    ))
}

async fn earliest_log_time(
    db: &DatabaseConnection,
    identity_id: &str,
) -> Result<Option<DateTime<Utc>>, StorageError> {
    #[derive(FromQueryResult)]
    struct Earliest {
        created_at: Option<String>,
    }

    let earliest = identity_activities::Entity::find()
        .filter(identity_activities::Column::IdentityId.eq(identity_id))
        .select_only()
        .column_as(identity_activities::Column::CreatedAt.min(), "created_at")
        .into_model::<Earliest>()
        .one(db)
        .await?
        .and_then(|row| row.created_at);
    earliest
        .map(|value| {
            DateTime::parse_from_rfc3339(&value)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|error| StorageError::InvalidTimestamp(error.to_string()))
        })
        .transpose()
}

#[derive(FromQueryResult)]
struct BucketTokenAggregate {
    bucket: String,
    input_tokens: Option<i64>,
    total_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
}

impl BucketTokenAggregate {
    fn into_totals(self) -> UsageTotals {
        UsageTotals {
            input_tokens: self.input_tokens.unwrap_or_default(),
            total_input_tokens: self.total_input_tokens.unwrap_or_default(),
            output_tokens: self.output_tokens.unwrap_or_default(),
            cache_read_tokens: self.cache_read_tokens.unwrap_or_default(),
            cache_write_tokens: self.cache_write_tokens.unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct UsageTotals {
    input_tokens: i64,
    total_input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

impl UsageTotals {
    fn add(&mut self, totals: Self) {
        self.input_tokens += totals.input_tokens;
        self.total_input_tokens += totals.total_input_tokens;
        self.output_tokens += totals.output_tokens;
        self.cache_read_tokens += totals.cache_read_tokens;
        self.cache_write_tokens += totals.cache_write_tokens;
    }

    fn into_point(self, bucket: String) -> TokenUsageTimelinePoint {
        TokenUsageTimelinePoint {
            bucket,
            input_tokens: self.input_tokens,
            total_input_tokens: self.total_input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
        }
    }
}
