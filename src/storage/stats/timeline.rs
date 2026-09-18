use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter, QuerySelect,
};

use crate::{
    entity::identity::activities as identity_activities,
    stats::{
        all_timeline_bounds, timeline_buckets, StatsRange, TimeBounds, TokenUsageTimelinePoint,
    },
    storage::{Storage, StorageError},
};

use super::{aggregate_query, integer_sum, total_input_tokens_expr};

impl Storage {
    pub async fn token_usage_timeline(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
        timeline(&self.db, identity_id, range).await
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
        points.push(TokenUsageTimelinePoint {
            bucket: bucket.label,
            input_tokens: totals.input_tokens.unwrap_or_default(),
            total_input_tokens: totals.total_input_tokens.unwrap_or_default(),
            output_tokens: totals.output_tokens.unwrap_or_default(),
            cache_read_tokens: totals.cache_read_tokens.unwrap_or_default(),
            cache_write_tokens: totals.cache_write_tokens.unwrap_or_default(),
        });
    }
    Ok(points)
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

#[derive(Default, FromQueryResult)]
struct TokenAggregate {
    input_tokens: Option<i64>,
    total_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
}
