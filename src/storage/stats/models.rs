use chrono::Utc;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QuerySelect, RelationTrait, Select,
};

use crate::{
    entity::{identities, identity::activities as identity_activities},
    provider_catalog::ProviderCatalog,
    stats::{ModelStatsScope, ModelStatsSummary, StatsRange, TimeBounds},
    storage::{Storage, StorageError},
};

use super::{
    aggregate_query, failed_count_expr, floating_average, integer_sum, success_count_expr,
};

impl Storage {
    pub async fn model_stats(
        &self,
        identity_id: &str,
        scope: ModelStatsScope,
        range: StatsRange,
    ) -> Result<Vec<ModelStatsSummary>, StorageError> {
        list_model_stats(&self.db, identity_id, scope, range, None).await
    }

    pub async fn model_stats_with_catalog(
        &self,
        identity_id: &str,
        scope: ModelStatsScope,
        range: StatsRange,
        catalog: &ProviderCatalog,
    ) -> Result<Vec<ModelStatsSummary>, StorageError> {
        list_model_stats(&self.db, identity_id, scope, range, Some(catalog)).await
    }
}

async fn list_model_stats(
    db: &DatabaseConnection,
    identity_id: &str,
    scope: ModelStatsScope,
    range: StatsRange,
    catalog: Option<&ProviderCatalog>,
) -> Result<Vec<ModelStatsSummary>, StorageError> {
    let bounds = range.bounds(Utc::now());
    let rows = match scope {
        ModelStatsScope::Personal => {
            list_model_aggregates(db, aggregate_query(identity_id, bounds)).await?
        }
        ModelStatsScope::Team => list_model_aggregates(db, team_aggregate_query(bounds)).await?,
    };
    Ok(model_summaries(rows, catalog))
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

/// personal 与 team 两条路径共用同一套聚合投影和分组，避免列定义漂移。
async fn list_model_aggregates<E>(
    db: &DatabaseConnection,
    query: Select<E>,
) -> Result<Vec<ModelAggregate>, StorageError>
where
    E: EntityTrait,
{
    Ok(query
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
        .await?)
}

/// 聚合结果 → 对外结构：解析目录显示名，并按请求数排序。
fn model_summaries(
    rows: Vec<ModelAggregate>,
    catalog: Option<&ProviderCatalog>,
) -> Vec<ModelStatsSummary> {
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
    summaries
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
