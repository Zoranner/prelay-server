use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QuerySelect, Select};

use crate::{
    entity::identity::activities as identity_activities,
    provider_catalog::ProviderCatalog,
    stats::{ModelStatsScope, ModelStatsSummary, StatsRange},
    storage::{Storage, StorageError},
};

use super::{
    aggregate_query, failed_count_expr, floating_average, integer_sum, success_count_expr,
    team_aggregate_query,
};

const DELETED_MODEL_NAME: &str = "已删除模型";

impl Storage {
    pub async fn model_stats_with_catalog(
        &self,
        identity_id: &str,
        scope: ModelStatsScope,
        range: StatsRange,
        catalog: &ProviderCatalog,
    ) -> Result<Vec<ModelStatsSummary>, StorageError> {
        list_model_stats(&self.db, identity_id, scope, range, catalog).await
    }
}

async fn list_model_stats(
    db: &DatabaseConnection,
    identity_id: &str,
    scope: ModelStatsScope,
    range: StatsRange,
    catalog: &ProviderCatalog,
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

/// 聚合结果 → 对外结构：当前目录内的模型按 ID 展示当前名称；
/// 已从目录删除的模型合并成一行，保留请求数和用量，并放在最后。
fn model_summaries(rows: Vec<ModelAggregate>, catalog: &ProviderCatalog) -> Vec<ModelStatsSummary> {
    let mut summaries = Vec::with_capacity(rows.len());
    let mut deleted = ModelTotals::default();
    for row in rows {
        if row
            .model_requested
            .as_deref()
            .is_some_and(|model_id| catalog.model_exists(model_id))
        {
            summaries.push(row.into_summary(catalog));
        } else {
            deleted.add(row);
        }
    }
    summaries.sort_by(|left, right| {
        right
            .total_requests
            .cmp(&left.total_requests)
            .then_with(|| left.model_requested.cmp(&right.model_requested))
    });
    if deleted.total_requests > 0 {
        summaries.push(deleted.into_summary());
    }
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

impl ModelAggregate {
    fn into_summary(self, catalog: &ProviderCatalog) -> ModelStatsSummary {
        let display_name = self
            .model_requested
            .as_deref()
            .map(|model_id| catalog.model_display_name(model_id));
        ModelStatsSummary {
            model_requested: self.model_requested,
            model_requested_display_name: display_name,
            total_requests: self.total_requests,
            successful_requests: self.successful_requests.unwrap_or_default(),
            failed_requests: self.failed_requests.unwrap_or_default(),
            input_tokens: self.input_tokens.unwrap_or_default(),
            output_tokens: self.output_tokens.unwrap_or_default(),
            average_latency_ms: floating_average(self.latency_total, self.latency_count),
        }
    }
}

#[derive(Default)]
struct ModelTotals {
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    latency_total: i64,
    latency_count: i64,
}

impl ModelTotals {
    fn add(&mut self, row: ModelAggregate) {
        self.total_requests += row.total_requests;
        self.successful_requests += row.successful_requests.unwrap_or_default();
        self.failed_requests += row.failed_requests.unwrap_or_default();
        self.input_tokens += row.input_tokens.unwrap_or_default();
        self.output_tokens += row.output_tokens.unwrap_or_default();
        self.latency_total += row.latency_total.unwrap_or_default();
        self.latency_count += row.latency_count;
    }

    fn into_summary(self) -> ModelStatsSummary {
        ModelStatsSummary {
            model_requested: None,
            model_requested_display_name: Some(DELETED_MODEL_NAME.to_string()),
            total_requests: self.total_requests,
            successful_requests: self.successful_requests,
            failed_requests: self.failed_requests,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            average_latency_ms: floating_average(Some(self.latency_total), self.latency_count),
        }
    }
}
