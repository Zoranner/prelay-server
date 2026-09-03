use chrono::{DateTime, Utc};
use sea_orm::{
    sea_query::{Expr, ExprTrait},
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter, QuerySelect,
    Select,
};

use crate::{
    entity::identity::activities as identity_activities,
    provider_catalog::ProviderCatalog,
    stats::{
        all_timeline_bounds, timeline_buckets, ModelStatsSummary, ProviderStatsSummary,
        StatsOverview, StatsRange, TimeBounds, TokenUsageTimelinePoint,
    },
    storage::{Storage, StorageError},
};

impl Storage {
    pub async fn stats_overview(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<StatsOverview, StorageError> {
        overview(&self.db, identity_id, range).await
    }

    pub async fn model_stats(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<ModelStatsSummary>, StorageError> {
        list_model_stats(&self.db, identity_id, range, None).await
    }

    pub async fn model_stats_with_catalog(
        &self,
        identity_id: &str,
        range: StatsRange,
        catalog: &ProviderCatalog,
    ) -> Result<Vec<ModelStatsSummary>, StorageError> {
        list_model_stats(&self.db, identity_id, range, Some(catalog)).await
    }

    pub async fn provider_stats(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<ProviderStatsSummary>, StorageError> {
        list_provider_stats(&self.db, identity_id, range).await
    }

    pub async fn token_usage_timeline(
        &self,
        identity_id: &str,
        range: StatsRange,
    ) -> Result<Vec<TokenUsageTimelinePoint>, StorageError> {
        timeline(&self.db, identity_id, range).await
    }
}

async fn overview(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
) -> Result<StatsOverview, StorageError> {
    let row = aggregate_query(identity_id, range.bounds(Utc::now()))
        .select_only()
        .column_as(identity_activities::Column::Id.count(), "total_requests")
        .column_as(success_count_expr(), "successful_requests")
        .column_as(failed_count_expr(), "failed_requests")
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
        .column_as(
            integer_sum(identity_activities::Column::LatencyMs.sum()),
            "latency_total",
        )
        .column_as(
            identity_activities::Column::LatencyMs.count(),
            "latency_count",
        )
        .into_model::<OverviewAggregate>()
        .one(db)
        .await?
        .unwrap_or_default();
    Ok(StatsOverview {
        total_requests: row.total_requests,
        successful_requests: row.successful_requests.unwrap_or_default(),
        failed_requests: row.failed_requests.unwrap_or_default(),
        input_tokens: row.input_tokens.unwrap_or_default(),
        total_input_tokens: row.total_input_tokens.unwrap_or_default(),
        output_tokens: row.output_tokens.unwrap_or_default(),
        cache_read_tokens: row.cache_read_tokens.unwrap_or_default(),
        cache_write_tokens: row.cache_write_tokens.unwrap_or_default(),
        average_latency_ms: integer_average(row.latency_total, row.latency_count),
    })
}

async fn list_model_stats(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
    catalog: Option<&ProviderCatalog>,
) -> Result<Vec<ModelStatsSummary>, StorageError> {
    let rows = aggregate_query(identity_id, range.bounds(Utc::now()))
        .select_only()
        .column(identity_activities::Column::ModelRequested)
        .column_as(identity_activities::Column::Id.count(), "total_requests")
        .column_as(success_count_expr(), "successful_requests")
        .column_as(failed_count_expr(), "failed_requests")
        .column_as(
            integer_sum(identity_activities::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(
            integer_sum(identity_activities::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(
            integer_sum(identity_activities::Column::LatencyMs.sum()),
            "latency_total",
        )
        .column_as(
            identity_activities::Column::LatencyMs.count(),
            "latency_count",
        )
        .group_by(identity_activities::Column::ModelRequested)
        .into_model::<ModelAggregate>()
        .all(db)
        .await?;
    let mut summaries =
        rows.into_iter()
            .map(|row| ModelStatsSummary {
                model_requested_display_name: row.model_requested.as_deref().and_then(|model_id| {
                    catalog.map(|catalog| catalog.model_display_name(model_id))
                }),
                model_requested: row.model_requested,
                total_requests: row.total_requests,
                successful_requests: row.successful_requests.unwrap_or_default(),
                failed_requests: row.failed_requests.unwrap_or_default(),
                input_tokens: row.input_tokens.unwrap_or_default(),
                output_tokens: row.output_tokens.unwrap_or_default(),
                average_latency_ms: floating_average(row.latency_total, row.latency_count),
            })
            .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .total_requests
            .cmp(&left.total_requests)
            .then_with(|| left.model_requested.cmp(&right.model_requested))
    });
    Ok(summaries)
}

async fn list_provider_stats(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
) -> Result<Vec<ProviderStatsSummary>, StorageError> {
    let rows = aggregate_query(identity_id, range.bounds(Utc::now()))
        .select_only()
        .column(identity_activities::Column::ProviderId)
        .column(identity_activities::Column::ProviderName)
        .column_as(identity_activities::Column::Id.count(), "total_requests")
        .column_as(success_count_expr(), "successful_requests")
        .column_as(failed_count_expr(), "failed_requests")
        .column_as(
            integer_sum(identity_activities::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(
            integer_sum(identity_activities::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(
            integer_sum(identity_activities::Column::LatencyMs.sum()),
            "latency_total",
        )
        .column_as(
            identity_activities::Column::LatencyMs.count(),
            "latency_count",
        )
        .column_as(
            integer_sum(identity_activities::Column::FirstTokenMs.sum()),
            "first_token_total",
        )
        .column_as(
            identity_activities::Column::FirstTokenMs.count(),
            "first_token_count",
        )
        .group_by(identity_activities::Column::ProviderId)
        .group_by(identity_activities::Column::ProviderName)
        .into_model::<ProviderAggregate>()
        .all(db)
        .await?;
    let mut summaries = rows
        .into_iter()
        .map(|row| ProviderStatsSummary {
            provider_id: row.provider_id,
            provider_name: row.provider_name,
            total_requests: row.total_requests,
            successful_requests: row.successful_requests.unwrap_or_default(),
            failed_requests: row.failed_requests.unwrap_or_default(),
            input_tokens: row.input_tokens.unwrap_or_default(),
            output_tokens: row.output_tokens.unwrap_or_default(),
            average_latency_ms: floating_average(row.latency_total, row.latency_count),
            average_first_token_ms: floating_average(row.first_token_total, row.first_token_count),
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .total_requests
            .cmp(&left.total_requests)
            .then_with(|| left.provider_name.cmp(&right.provider_name))
    });
    Ok(summaries)
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

fn aggregate_query(
    identity_id: &str,
    bounds: Option<TimeBounds>,
) -> Select<identity_activities::Entity> {
    let mut query = identity_activities::Entity::find()
        .filter(identity_activities::Column::IdentityId.eq(identity_id));
    if let Some(bounds) = bounds {
        query = query
            .filter(identity_activities::Column::CreatedAt.gte(bounds.start.to_rfc3339()))
            .filter(identity_activities::Column::CreatedAt.lt(bounds.end.to_rfc3339()));
    }
    query
}

fn success_count_expr() -> Expr {
    let case: Expr = Expr::case(
        Expr::col(identity_activities::Column::Status).eq("success"),
        1,
    )
    .finally(0)
    .into();
    case.sum()
}

fn failed_count_expr() -> Expr {
    let case: Expr = Expr::case(
        Expr::col(identity_activities::Column::Status).ne("success"),
        1,
    )
    .finally(0)
    .into();
    case.sum()
}

fn integer_sum(expr: Expr) -> Expr {
    expr.cast_as("bigint")
}

fn total_input_tokens_expr() -> Expr {
    let input = Expr::col(identity_activities::Column::InputTokens).if_null(0);
    let cache_read = Expr::col(identity_activities::Column::CacheReadTokens).if_null(0);
    let cache_write = Expr::col(identity_activities::Column::CacheWriteTokens).if_null(0);
    let use_reported_input = Expr::col(identity_activities::Column::ProtocolUpstream)
        .is_in(["responses", "chat_completions"])
        .and(input.clone().gte(cache_read.clone()));
    let normalized: Expr = Expr::case(use_reported_input, input.clone())
        .finally(input.add(cache_read))
        .into();
    integer_sum(normalized.add(cache_write).sum())
}

fn integer_average(total: Option<i64>, count: i64) -> Option<i64> {
    (count > 0).then(|| total.unwrap_or_default() / count)
}

fn floating_average(total: Option<i64>, count: i64) -> Option<f64> {
    (count > 0).then(|| total.unwrap_or_default() as f64 / count as f64)
}

#[derive(Default, FromQueryResult)]
struct OverviewAggregate {
    total_requests: i64,
    successful_requests: Option<i64>,
    failed_requests: Option<i64>,
    input_tokens: Option<i64>,
    total_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    latency_total: Option<i64>,
    latency_count: i64,
}

#[derive(FromQueryResult)]
struct ModelAggregate {
    model_requested: Option<String>,
    total_requests: i64,
    successful_requests: Option<i64>,
    failed_requests: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    latency_total: Option<i64>,
    latency_count: i64,
}

#[derive(FromQueryResult)]
struct ProviderAggregate {
    provider_id: Option<String>,
    provider_name: Option<String>,
    total_requests: i64,
    successful_requests: Option<i64>,
    failed_requests: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    latency_total: Option<i64>,
    latency_count: i64,
    first_token_total: Option<i64>,
    first_token_count: i64,
}

#[derive(Default, FromQueryResult)]
struct TokenAggregate {
    input_tokens: Option<i64>,
    total_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
}

#[cfg(test)]
#[path = "stats/tests.rs"]
mod tests;
