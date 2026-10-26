use chrono::Utc;
use sea_orm::{
    sea_query::{Expr, ExprTrait},
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QuerySelect, RelationTrait, Select,
};

use crate::{
    entity::{identities, identity::activities as identity_activities},
    provider_catalog::ProviderCatalog,
    stats::{ModelStatsScope, ProviderStatsSummary, StatsOverview, StatsRange, TimeBounds},
    storage::{Storage, StorageError},
};

mod models;
mod timeline;

const DELETED_PROVIDER_NAME: &str = "已删除供应商";

impl Storage {
    pub async fn stats_overview(
        &self,
        identity_id: &str,
        scope: ModelStatsScope,
        range: StatsRange,
    ) -> Result<StatsOverview, StorageError> {
        overview(&self.db, identity_id, scope, range).await
    }

    pub async fn provider_stats(
        &self,
        identity_id: &str,
        scope: ModelStatsScope,
        range: StatsRange,
        catalog: &ProviderCatalog,
    ) -> Result<Vec<ProviderStatsSummary>, StorageError> {
        list_provider_stats(&self.db, identity_id, scope, range, catalog).await
    }
}

/// 全站口径：与用户排行榜一致，从 identities 与活动表内连接出发，
/// 不按身份过滤，只保留请求时间范围条件。
fn team_aggregate_query(bounds: Option<TimeBounds>) -> Select<identities::Entity> {
    let mut query = identities::Entity::find()
        .join(JoinType::InnerJoin, identities::Relation::Activities.def());
    if let Some(bounds) = bounds {
        query = query
            .filter(identity_activities::Column::CreatedAt.gte(bounds.start.to_rfc3339()))
            .filter(identity_activities::Column::CreatedAt.lt(bounds.end.to_rfc3339()));
    }
    query
}

async fn overview(
    db: &DatabaseConnection,
    identity_id: &str,
    scope: ModelStatsScope,
    range: StatsRange,
) -> Result<StatsOverview, StorageError> {
    let bounds = range.bounds(Utc::now());
    let row = match scope {
        ModelStatsScope::Personal => {
            overview_aggregate(db, aggregate_query(identity_id, bounds)).await?
        }
        ModelStatsScope::Team => overview_aggregate(db, team_aggregate_query(bounds)).await?,
    };
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

/// personal 与 team 两条路径共用同一套总览聚合投影。
async fn overview_aggregate<E>(
    db: &DatabaseConnection,
    query: Select<E>,
) -> Result<OverviewAggregate, StorageError>
where
    E: EntityTrait,
{
    Ok(query
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
        .unwrap_or_default())
}

async fn list_provider_stats(
    db: &DatabaseConnection,
    identity_id: &str,
    scope: ModelStatsScope,
    range: StatsRange,
    catalog: &ProviderCatalog,
) -> Result<Vec<ProviderStatsSummary>, StorageError> {
    let bounds = range.bounds(Utc::now());
    let rows = match scope {
        ModelStatsScope::Personal => {
            provider_aggregate_rows(db, aggregate_query(identity_id, bounds)).await?
        }
        ModelStatsScope::Team => provider_aggregate_rows(db, team_aggregate_query(bounds)).await?,
    };
    let mut summaries = Vec::with_capacity(rows.len());
    let mut deleted = ProviderTotals::default();
    for row in rows {
        let provider = row
            .provider_type
            .as_deref()
            .and_then(|provider_type| catalog.provider(provider_type));
        match provider {
            Some(provider) => {
                let provider_type = row.provider_type.clone();
                let provider_name = provider.name.clone();
                summaries.push(row.into_summary(provider_type, provider_name));
            }
            None => deleted.add(row),
        }
    }
    summaries.sort_by(|left, right| {
        right
            .total_requests
            .cmp(&left.total_requests)
            .then_with(|| left.provider_name.cmp(&right.provider_name))
    });
    if deleted.total_requests > 0 {
        summaries.push(deleted.into_summary(None, Some(DELETED_PROVIDER_NAME.to_string())));
    }
    Ok(summaries)
}

/// personal 与 team 两条路径共用同一套供应商聚合投影和分组。
async fn provider_aggregate_rows<E>(
    db: &DatabaseConnection,
    query: Select<E>,
) -> Result<Vec<ProviderAggregate>, StorageError>
where
    E: EntityTrait,
{
    Ok(query
        .select_only()
        .column(identity_activities::Column::ProviderType)
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
        .group_by(identity_activities::Column::ProviderType)
        .into_model::<ProviderAggregate>()
        .all(db)
        .await?)
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
        Expr::col(identity_activities::Column::Status).is_in(["failed", "downstream_disconnected"]),
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
struct ProviderAggregate {
    provider_type: Option<String>,
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

impl ProviderAggregate {
    fn into_summary(
        self,
        provider_type: Option<String>,
        provider_name: String,
    ) -> ProviderStatsSummary {
        ProviderTotals::from(self).into_summary(provider_type, Some(provider_name))
    }
}

#[derive(Default)]
struct ProviderTotals {
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    latency_total: i64,
    latency_count: i64,
    first_token_total: i64,
    first_token_count: i64,
}

impl From<ProviderAggregate> for ProviderTotals {
    fn from(row: ProviderAggregate) -> Self {
        Self {
            total_requests: row.total_requests,
            successful_requests: row.successful_requests.unwrap_or_default(),
            failed_requests: row.failed_requests.unwrap_or_default(),
            input_tokens: row.input_tokens.unwrap_or_default(),
            output_tokens: row.output_tokens.unwrap_or_default(),
            latency_total: row.latency_total.unwrap_or_default(),
            latency_count: row.latency_count,
            first_token_total: row.first_token_total.unwrap_or_default(),
            first_token_count: row.first_token_count,
        }
    }
}

impl ProviderTotals {
    fn add(&mut self, row: ProviderAggregate) {
        let totals = Self::from(row);
        self.total_requests += totals.total_requests;
        self.successful_requests += totals.successful_requests;
        self.failed_requests += totals.failed_requests;
        self.input_tokens += totals.input_tokens;
        self.output_tokens += totals.output_tokens;
        self.latency_total += totals.latency_total;
        self.latency_count += totals.latency_count;
        self.first_token_total += totals.first_token_total;
        self.first_token_count += totals.first_token_count;
    }

    fn into_summary(
        self,
        provider_type: Option<String>,
        provider_name: Option<String>,
    ) -> ProviderStatsSummary {
        ProviderStatsSummary {
            provider_type,
            provider_name,
            total_requests: self.total_requests,
            successful_requests: self.successful_requests,
            failed_requests: self.failed_requests,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            average_latency_ms: floating_average(Some(self.latency_total), self.latency_count),
            average_first_token_ms: floating_average(
                Some(self.first_token_total),
                self.first_token_count,
            ),
        }
    }
}

#[cfg(test)]
#[path = "stats/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "stats/timeline_tests.rs"]
mod timeline_tests;
