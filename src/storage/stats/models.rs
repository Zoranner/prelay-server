use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, FromQueryResult, QuerySelect};

use crate::{
    entity::identity::activities as identity_activities,
    provider_catalog::ProviderCatalog,
    stats::{ModelStatsSummary, StatsRange},
    storage::{Storage, StorageError},
};

use super::{
    aggregate_query, failed_count_expr, floating_average, integer_sum, success_count_expr,
};

impl Storage {
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
