use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sea_orm::{
    sea_query::Expr, ColumnTrait, DatabaseConnection, DbBackend, EntityTrait, FromQueryResult,
    QueryFilter, QuerySelect,
};

use crate::{
    entity::identity::activities as identity_activities,
    stats::{
        all_daily_bounds, all_timeline_bounds, timeline_buckets, StatsRange, TimeBounds,
        TimelineGranularity, TokenUsageTimelinePoint,
    },
    storage::{Storage, StorageError},
};

use super::{aggregate_query, integer_sum, total_input_tokens_expr};

/// 按天粒度一次最多返回的自然日数量：覆盖一整年（366 天）并留出余量，
/// 更宽的窗口（例如 range=all 的历史超过 400 天）按非法请求拒绝。
const MAX_DAILY_BUCKETS: usize = 400;

impl Storage {
    pub async fn token_usage_timeline(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
        timeline(&self.db, identity_id, range).await
    }

    /// 按北京时间自然日聚合的时间线：一次分组查询取回有数据的天，再在内存里补零。
    pub async fn daily_token_usage_timeline(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
        daily_timeline(&self.db, identity_id, range).await
    }
}

async fn timeline(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
    let now = Utc::now();
    let bounds = match range.bounds(now) {
        Some(bounds) => bounds,
        None => {
            let Some(earliest) = earliest_log_time(db, identity_id).await? else {
                return Ok(Vec::new());
            };
            all_timeline_bounds(earliest, now)
        }
    };
    let mut points = Vec::new();
    for bucket in timeline_buckets(bounds, range.timeline_granularity()) {
        let totals = token_totals(db, identity_id, bucket.bounds).await?;
        points.push(totals.into_point(bucket.label));
    }
    Ok(points)
}

async fn daily_timeline(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
    let now = Utc::now();
    let bounds = match range.bounds(now) {
        Some(bounds) => bounds,
        None => match earliest_log_time(db, identity_id).await? {
            Some(earliest) => all_daily_bounds(earliest, now),
            None => return Ok(Vec::new()),
        },
    };
    let buckets = timeline_buckets(bounds, TimelineGranularity::Day);
    if buckets.len() > MAX_DAILY_BUCKETS {
        return Err(StorageError::ValidationFailed(format!(
            "granularity=day supports at most {MAX_DAILY_BUCKETS} buckets, requested {}",
            buckets.len()
        )));
    }
    let mut totals = daily_token_totals(db, identity_id, bounds)
        .await?
        .into_iter()
        .map(|row| {
            let bucket = row.bucket.clone();
            (bucket, row.into_totals())
        })
        .collect::<HashMap<_, _>>();
    Ok(buckets
        .into_iter()
        .map(|bucket| {
            totals
                .remove(&bucket.label)
                .unwrap_or_default()
                .into_point(bucket.label)
        })
        .collect())
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

async fn token_totals(
    db: &DatabaseConnection,
    identity_id: &str,
    bounds: TimeBounds,
) -> Result<TokenAggregate, StorageError> {
    Ok(aggregate_query(identity_id, Some(bounds))
        .select_only()
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
        .into_model::<TokenAggregate>()
        .one(db)
        .await?
        .unwrap_or_default())
}

/// 按天粒度的一次分组查询：日期由 SQL 按北京时间切分，只返回有数据的天。
async fn daily_token_totals(
    db: &DatabaseConnection,
    identity_id: &str,
    bounds: TimeBounds,
) -> Result<Vec<DailyTokenAggregate>, StorageError> {
    let day = beijing_day_expr(db.get_database_backend());
    Ok(aggregate_query(identity_id, Some(bounds))
        .select_only()
        .expr_as(day.clone(), "bucket")
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
        .group_by(day)
        .into_model::<DailyTokenAggregate>()
        .all(db)
        .await?)
}

/// 北京时间自然日的分组表达式。created_at 以 RFC3339 UTC 文本保存，
/// SQLite 与 PostgreSQL 的日期函数不同，这里按后端分别生成。
fn beijing_day_expr(backend: DbBackend) -> Expr {
    match backend {
        DbBackend::Postgres => Expr::cust(
            "to_char((identity_activities.created_at::timestamptz AT TIME ZONE 'UTC') + interval '8 hours', 'YYYY-MM-DD')",
        ),
        _ => Expr::cust("strftime('%Y-%m-%d', identity_activities.created_at, '+8 hours')"),
    }
}

#[derive(Default, FromQueryResult)]
struct TokenAggregate {
    input_tokens: Option<i64>,
    total_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
}

impl TokenAggregate {
    fn into_point(self, bucket: String) -> TokenUsageTimelinePoint {
        TokenUsageTimelinePoint {
            bucket,
            input_tokens: self.input_tokens.unwrap_or_default(),
            total_input_tokens: self.total_input_tokens.unwrap_or_default(),
            output_tokens: self.output_tokens.unwrap_or_default(),
            cache_read_tokens: self.cache_read_tokens.unwrap_or_default(),
            cache_write_tokens: self.cache_write_tokens.unwrap_or_default(),
        }
    }
}

#[derive(FromQueryResult)]
struct DailyTokenAggregate {
    bucket: String,
    input_tokens: Option<i64>,
    total_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
}

impl DailyTokenAggregate {
    fn into_totals(self) -> TokenAggregate {
        TokenAggregate {
            input_tokens: self.input_tokens,
            total_input_tokens: self.total_input_tokens,
            output_tokens: self.output_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
        }
    }
}
