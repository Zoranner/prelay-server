use chrono::{DateTime, Utc};
use sea_orm::{
    sea_query::{Expr, ExprTrait},
    ActiveModelTrait,
    ActiveValue::Set,
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter, QueryOrder,
    QuerySelect, Select,
};
use uuid::Uuid;

use crate::{
    entity::identity_request_logs,
    stats::{
        all_timeline_bounds, estimate_cost, load_model_prices, timeline_buckets, ModelPrice,
        ModelStatsSummary, ProviderStatsSummary, RequestLogInsert, RequestLogSummary,
        StatsOverview, StatsRange, StreamRequestLogUpdate, TimeBounds, TokenUsageTimelinePoint,
    },
    storage::StorageError,
};

pub(super) async fn insert(
    db: &DatabaseConnection,
    identity_id: &str,
    log: RequestLogInsert,
) -> Result<(), StorageError> {
    insert_with_id(db, identity_id, Uuid::new_v4().to_string(), log).await
}

pub(super) async fn insert_with_id(
    db: &DatabaseConnection,
    identity_id: &str,
    id: String,
    log: RequestLogInsert,
) -> Result<(), StorageError> {
    let prices = load_model_prices().unwrap_or_default();
    insert_with_id_and_prices(db, identity_id, id, log, &prices).await
}

async fn insert_with_id_and_prices(
    db: &DatabaseConnection,
    identity_id: &str,
    id: String,
    log: RequestLogInsert,
    prices: &[ModelPrice],
) -> Result<(), StorageError> {
    let cost = estimate_cost(&log, prices);
    identity_request_logs::ActiveModel {
        id: Set(id),
        identity_id: Set(identity_id.to_string()),
        created_at: Set(Utc::now().to_rfc3339()),
        protocol_in: Set(Some(log.protocol_in)),
        protocol_out: Set(Some(log.protocol_out)),
        protocol_upstream: Set(Some(log.protocol_upstream)),
        endpoint_name: Set(Some(log.endpoint_name)),
        provider_id: Set(Some(log.provider_id)),
        provider_name: Set(Some(log.provider_name)),
        model_requested: Set(Some(log.model_requested)),
        model_upstream: Set(Some(log.model_upstream)),
        proxy_token_id: Set(None),
        status: Set(log.status),
        http_status: Set(Some(log.http_status)),
        error_code: Set(log.error_code),
        error_message: Set(log.error_message),
        is_streaming: Set(Some(log.is_streaming)),
        input_tokens: Set(log.input_tokens),
        output_tokens: Set(log.output_tokens),
        reasoning_tokens: Set(log.reasoning_tokens),
        cache_read_tokens: Set(log.cache_read_tokens),
        cache_write_tokens: Set(log.cache_write_tokens),
        estimated_cost: Set(cost.as_ref().map(|cost| cost.estimated_cost)),
        currency: Set(cost.map(|cost| cost.currency)),
        latency_ms: Set(Some(log.latency_ms)),
        upstream_latency_ms: Set(log.upstream_latency_ms),
        first_token_ms: Set(log.first_token_ms),
        tool_call_count: Set(log.tool_call_count),
        upstream_request_id: Set(log.upstream_request_id),
        metadata_json: Set(log.metadata_json),
    }
    .insert(db)
    .await?;
    Ok(())
}

pub(super) async fn update_stream(
    db: &DatabaseConnection,
    identity_id: &str,
    id: &str,
    update: StreamRequestLogUpdate,
) -> Result<(), StorageError> {
    let prices = load_model_prices().unwrap_or_default();
    update_stream_with_prices(db, identity_id, id, update, &prices).await
}

async fn update_stream_with_prices(
    db: &DatabaseConnection,
    identity_id: &str,
    id: &str,
    update: StreamRequestLogUpdate,
    prices: &[ModelPrice],
) -> Result<(), StorageError> {
    let row = identity_request_logs::Entity::find_by_id(id)
        .filter(identity_request_logs::Column::IdentityId.eq(identity_id))
        .one(db)
        .await?
        .ok_or(StorageError::RequestLogNotFound)?;
    let pricing_log = RequestLogInsert {
        provider_name: row.provider_name.clone().unwrap_or_default(),
        model_requested: row.model_requested.clone().unwrap_or_default(),
        model_upstream: row.model_upstream.clone().unwrap_or_default(),
        status: update.status.clone(),
        http_status: update.http_status,
        error_code: update.error_code.clone(),
        error_message: update.error_message.clone(),
        is_streaming: true,
        input_tokens: update.input_tokens,
        output_tokens: update.output_tokens,
        reasoning_tokens: update.reasoning_tokens,
        cache_read_tokens: update.cache_read_tokens,
        cache_write_tokens: update.cache_write_tokens,
        latency_ms: update.latency_ms,
        tool_call_count: update.tool_call_count,
        upstream_request_id: update.upstream_request_id.clone(),
        metadata_json: update.metadata_json.clone(),
        ..Default::default()
    };
    let cost = estimate_cost(&pricing_log, prices);
    let mut active: identity_request_logs::ActiveModel = row.into();
    active.status = Set(update.status);
    active.http_status = Set(Some(update.http_status));
    active.error_code = Set(update.error_code);
    active.error_message = Set(update.error_message);
    active.input_tokens = Set(update.input_tokens);
    active.output_tokens = Set(update.output_tokens);
    active.reasoning_tokens = Set(update.reasoning_tokens);
    active.cache_read_tokens = Set(update.cache_read_tokens);
    active.cache_write_tokens = Set(update.cache_write_tokens);
    active.estimated_cost = Set(cost.as_ref().map(|cost| cost.estimated_cost));
    active.currency = Set(cost.map(|cost| cost.currency));
    active.latency_ms = Set(Some(update.latency_ms));
    active.tool_call_count = Set(update.tool_call_count);
    if update.upstream_request_id.is_some() {
        active.upstream_request_id = Set(update.upstream_request_id);
    }
    active.metadata_json = Set(update.metadata_json);
    active.update(db).await?;
    Ok(())
}

pub(super) async fn overview(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
) -> Result<StatsOverview, StorageError> {
    let row = aggregate_query(identity_id, range.bounds(Utc::now()))
        .select_only()
        .column_as(identity_request_logs::Column::Id.count(), "total_requests")
        .column_as(success_count_expr(), "successful_requests")
        .column_as(failed_count_expr(), "failed_requests")
        .column_as(
            integer_sum(identity_request_logs::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(total_input_tokens_expr(), "total_input_tokens")
        .column_as(
            integer_sum(identity_request_logs::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::CacheReadTokens.sum()),
            "cache_read_tokens",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::CacheWriteTokens.sum()),
            "cache_write_tokens",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::LatencyMs.sum()),
            "latency_total",
        )
        .column_as(
            identity_request_logs::Column::LatencyMs.count(),
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

pub(super) async fn list_requests(
    db: &DatabaseConnection,
    identity_id: &str,
    limit: usize,
) -> Result<Vec<RequestLogSummary>, StorageError> {
    let rows = identity_request_logs::Entity::find()
        .filter(identity_request_logs::Column::IdentityId.eq(identity_id))
        .order_by_desc(identity_request_logs::Column::CreatedAt)
        .limit(limit.min(500) as u64)
        .all(db)
        .await?;
    Ok(rows.into_iter().map(request_summary).collect())
}

pub(super) async fn list_model_stats(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
) -> Result<Vec<ModelStatsSummary>, StorageError> {
    let rows = aggregate_query(identity_id, range.bounds(Utc::now()))
        .select_only()
        .column(identity_request_logs::Column::ModelRequested)
        .column_as(identity_request_logs::Column::Id.count(), "total_requests")
        .column_as(success_count_expr(), "successful_requests")
        .column_as(failed_count_expr(), "failed_requests")
        .column_as(
            integer_sum(identity_request_logs::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(
            identity_request_logs::Column::EstimatedCost.sum(),
            "estimated_cost",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::LatencyMs.sum()),
            "latency_total",
        )
        .column_as(
            identity_request_logs::Column::LatencyMs.count(),
            "latency_count",
        )
        .group_by(identity_request_logs::Column::ModelRequested)
        .into_model::<ModelAggregate>()
        .all(db)
        .await?;
    let mut summaries = rows
        .into_iter()
        .map(|row| ModelStatsSummary {
            model_requested: row.model_requested,
            total_requests: row.total_requests,
            successful_requests: row.successful_requests.unwrap_or_default(),
            failed_requests: row.failed_requests.unwrap_or_default(),
            input_tokens: row.input_tokens.unwrap_or_default(),
            output_tokens: row.output_tokens.unwrap_or_default(),
            estimated_cost: row.estimated_cost,
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

pub(super) async fn list_provider_stats(
    db: &DatabaseConnection,
    identity_id: &str,
    range: StatsRange,
) -> Result<Vec<ProviderStatsSummary>, StorageError> {
    let rows = aggregate_query(identity_id, range.bounds(Utc::now()))
        .select_only()
        .column(identity_request_logs::Column::ProviderId)
        .column(identity_request_logs::Column::ProviderName)
        .column_as(identity_request_logs::Column::Id.count(), "total_requests")
        .column_as(success_count_expr(), "successful_requests")
        .column_as(failed_count_expr(), "failed_requests")
        .column_as(
            integer_sum(identity_request_logs::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(
            identity_request_logs::Column::EstimatedCost.sum(),
            "estimated_cost",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::LatencyMs.sum()),
            "latency_total",
        )
        .column_as(
            identity_request_logs::Column::LatencyMs.count(),
            "latency_count",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::FirstTokenMs.sum()),
            "first_token_total",
        )
        .column_as(
            identity_request_logs::Column::FirstTokenMs.count(),
            "first_token_count",
        )
        .group_by(identity_request_logs::Column::ProviderId)
        .group_by(identity_request_logs::Column::ProviderName)
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
            estimated_cost: row.estimated_cost,
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

pub(super) async fn timeline(
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

    let earliest = identity_request_logs::Entity::find()
        .filter(identity_request_logs::Column::IdentityId.eq(identity_id))
        .select_only()
        .column_as(identity_request_logs::Column::CreatedAt.min(), "created_at")
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
            integer_sum(identity_request_logs::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(total_input_tokens_expr(), "total_input_tokens")
        .column_as(
            integer_sum(identity_request_logs::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::CacheReadTokens.sum()),
            "cache_read_tokens",
        )
        .column_as(
            integer_sum(identity_request_logs::Column::CacheWriteTokens.sum()),
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
) -> Select<identity_request_logs::Entity> {
    let mut query = identity_request_logs::Entity::find()
        .filter(identity_request_logs::Column::IdentityId.eq(identity_id));
    if let Some(bounds) = bounds {
        query = query
            .filter(identity_request_logs::Column::CreatedAt.gte(bounds.start.to_rfc3339()))
            .filter(identity_request_logs::Column::CreatedAt.lt(bounds.end.to_rfc3339()));
    }
    query
}

fn success_count_expr() -> Expr {
    let case: Expr = Expr::case(
        Expr::col(identity_request_logs::Column::Status).eq("success"),
        1,
    )
    .finally(0)
    .into();
    case.sum()
}

fn failed_count_expr() -> Expr {
    let case: Expr = Expr::case(
        Expr::col(identity_request_logs::Column::Status).ne("success"),
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
    let input = Expr::col(identity_request_logs::Column::InputTokens).if_null(0);
    let cache_read = Expr::col(identity_request_logs::Column::CacheReadTokens).if_null(0);
    let cache_write = Expr::col(identity_request_logs::Column::CacheWriteTokens).if_null(0);
    let use_reported_input = Expr::col(identity_request_logs::Column::ProtocolUpstream)
        .is_in(["responses", "chat_completions"])
        .and(input.clone().gte(cache_read.clone()));
    let normalized: Expr = Expr::case(use_reported_input, input.clone())
        .finally(input.add(cache_read))
        .into();
    integer_sum(normalized.add(cache_write).sum())
}

fn request_summary(row: identity_request_logs::Model) -> RequestLogSummary {
    RequestLogSummary {
        id: row.id,
        created_at: row.created_at,
        protocol_in: row.protocol_in,
        protocol_upstream: row.protocol_upstream,
        endpoint_name: row.endpoint_name,
        provider_name: row.provider_name,
        model_requested: row.model_requested,
        status: row.status,
        http_status: row.http_status,
        error_code: row.error_code,
        error_message: row.error_message,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        is_streaming: row.is_streaming,
        first_token_ms: row.first_token_ms,
        cache_read_tokens: row.cache_read_tokens,
        cache_write_tokens: row.cache_write_tokens,
        latency_ms: row.latency_ms,
        upstream_request_id: row.upstream_request_id,
        metadata_json: row.metadata_json,
    }
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
    estimated_cost: Option<f64>,
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
    estimated_cost: Option<f64>,
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
mod tests {
    use sea_orm::{ColumnTrait, Database, EntityTrait, QueryFilter};

    use super::{insert_with_id_and_prices, update_stream_with_prices};
    use crate::{
        entity::identity_request_logs,
        schema::initialize,
        stats::{ModelPrice, RequestLogInsert, StatsRange, StreamRequestLogUpdate},
        storage::{MasterKey, Storage, StorageError},
    };

    #[tokio::test]
    async fn stream_update_changes_one_existing_identity_log_without_inserting() {
        let (storage, db) = test_storage().await;
        let identity = register_identity(&storage, "stream").await;
        let other_identity = register_identity(&storage, "other").await;
        storage
            .insert_request_log_with_id(&identity, "stream-log".to_string(), test_log(None, None))
            .await
            .expect("insert stream log");

        let error = storage
            .update_stream_request_log(
                &other_identity,
                "stream-log",
                StreamRequestLogUpdate {
                    status: "success".to_string(),
                    http_status: 200,
                    input_tokens: Some(99),
                    ..Default::default()
                },
            )
            .await
            .expect_err("another identity cannot update the log");
        assert!(matches!(error, StorageError::RequestLogNotFound));

        storage
            .update_stream_request_log(
                &identity,
                "stream-log",
                StreamRequestLogUpdate {
                    status: "success".to_string(),
                    http_status: 200,
                    input_tokens: Some(11),
                    output_tokens: Some(7),
                    latency_ms: 80,
                    upstream_request_id: Some("upstream-stream".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("update stream log");

        let rows = identity_request_logs::Entity::find()
            .filter(identity_request_logs::Column::Id.eq("stream-log"))
            .all(&db)
            .await
            .expect("load stream log");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity_id, identity);
        assert_eq!(rows[0].input_tokens, Some(11));
        assert_eq!(rows[0].output_tokens, Some(7));
        assert_eq!(rows[0].latency_ms, Some(80));
        assert_eq!(
            rows[0].upstream_request_id.as_deref(),
            Some("upstream-stream")
        );
    }

    #[tokio::test]
    async fn regular_insert_and_stream_completion_calculate_model_price() {
        let (storage, db) = test_storage().await;
        let identity = register_identity(&storage, "prices").await;
        let prices = [ModelPrice {
            provider: "Provider One".to_string(),
            model: "model-1".to_string(),
            input_price_per_1m: Some(1.0),
            output_price_per_1m: Some(2.0),
            currency: "USD".to_string(),
        }];
        insert_with_id_and_prices(
            &db,
            &identity,
            "regular-price".to_string(),
            test_log(Some(1_000_000), Some(500_000)),
            &prices,
        )
        .await
        .expect("insert priced regular log");
        insert_with_id_and_prices(
            &db,
            &identity,
            "stream-price".to_string(),
            test_log(None, None),
            &prices,
        )
        .await
        .expect("insert stream log");
        update_stream_with_prices(
            &db,
            &identity,
            "stream-price",
            StreamRequestLogUpdate {
                status: "success".to_string(),
                http_status: 200,
                input_tokens: Some(1_000_000),
                output_tokens: Some(500_000),
                ..Default::default()
            },
            &prices,
        )
        .await
        .expect("complete priced stream log");

        let rows = identity_request_logs::Entity::find()
            .filter(identity_request_logs::Column::IdentityId.eq(identity))
            .all(&db)
            .await
            .expect("load priced logs");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| {
            row.estimated_cost == Some(2.0) && row.currency.as_deref() == Some("USD")
        }));
    }

    #[tokio::test]
    async fn overview_is_scoped_to_one_identity() {
        let (storage, _) = test_storage().await;
        let identity_a = register_identity(&storage, "stats-a").await;
        let identity_b = register_identity(&storage, "stats-b").await;
        storage
            .insert_request_log_with_id(
                &identity_a,
                "stats-a-success".to_string(),
                test_log(Some(3), Some(4)),
            )
            .await
            .expect("insert identity A log");
        let mut failed = test_log(Some(50), Some(60));
        failed.status = "failed".to_string();
        failed.http_status = 502;
        storage
            .insert_request_log_with_id(&identity_b, "stats-b-failed".to_string(), failed)
            .await
            .expect("insert identity B log");

        let overview = storage
            .stats_overview(&identity_a, StatsRange::Today)
            .await
            .expect("load identity A overview");

        assert_eq!(overview.total_requests, 1);
        assert_eq!(overview.successful_requests, 1);
        assert_eq!(overview.failed_requests, 0);
        assert_eq!(overview.input_tokens, 3);
        assert_eq!(overview.output_tokens, 4);
    }

    #[tokio::test]
    async fn today_timeline_fills_empty_beijing_hour_buckets() {
        let (storage, _) = test_storage().await;
        let identity = register_identity(&storage, "timeline").await;
        storage
            .insert_request_log_with_id(
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

    async fn test_storage() -> (Storage, sea_orm::DatabaseConnection) {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect test database");
        initialize(&db).await.expect("initialize test schema");
        (
            Storage::from_connection(db.clone(), MasterKey::from_bytes([0; 32])),
            db,
        )
    }

    async fn register_identity(storage: &Storage, suffix: &str) -> String {
        storage
            .register_identity(
                &format!("machine-{suffix}"),
                &format!("sid-{suffix}"),
                &crate::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity")
            .identity_id
    }

    fn test_log(input_tokens: Option<i64>, output_tokens: Option<i64>) -> RequestLogInsert {
        RequestLogInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            endpoint_name: String::new(),
            provider_id: "provider-1".to_string(),
            provider_name: "Provider One".to_string(),
            model_requested: "model-1".to_string(),
            model_upstream: "model-1".to_string(),
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming: false,
            input_tokens,
            output_tokens,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            latency_ms: 10,
            upstream_latency_ms: None,
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
            metadata_json: None,
        }
    }
}
