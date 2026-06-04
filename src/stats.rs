use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct StatsOverview {
    pub total_requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RequestLogSummary {
    pub id: String,
    pub created_at: String,
    pub protocol_in: Option<String>,
    pub protocol_upstream: Option<String>,
    pub provider_name: Option<String>,
    pub model_requested: Option<String>,
    pub status: String,
    pub http_status: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ModelStatsSummary {
    pub model_requested: Option<String>,
    pub total_requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: Option<f64>,
    pub average_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ProviderStatsSummary {
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub total_requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost: Option<f64>,
    pub average_latency_ms: Option<f64>,
    pub average_first_token_ms: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RequestLogInsert {
    pub protocol_in: String,
    pub protocol_out: String,
    pub protocol_upstream: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_requested: String,
    pub model_upstream: String,
    pub status: String,
    pub http_status: i64,
    pub is_streaming: bool,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub latency_ms: i64,
    pub upstream_latency_ms: Option<i64>,
    pub first_token_ms: Option<i64>,
    pub tool_call_count: Option<i64>,
    pub upstream_request_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelPrice {
    pub provider: String,
    pub model: String,
    pub input_price_per_1m: Option<f64>,
    pub output_price_per_1m: Option<f64>,
    pub currency: String,
}

pub async fn insert_request_log(pool: &SqlitePool, log: RequestLogInsert) -> Result<()> {
    let prices = load_model_prices().unwrap_or_default();
    insert_request_log_with_prices(pool, log, &prices).await
}

async fn insert_request_log_with_prices(
    pool: &SqlitePool,
    log: RequestLogInsert,
    prices: &[ModelPrice],
) -> Result<()> {
    let cost = estimate_cost(&log, prices);
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            id,
            created_at,
            protocol_in,
            protocol_out,
            protocol_upstream,
            provider_id,
            provider_name,
            model_requested,
            model_upstream,
            status,
            http_status,
            is_streaming,
            input_tokens,
            output_tokens,
            reasoning_tokens,
            estimated_cost,
            currency,
            latency_ms,
            upstream_latency_ms,
            first_token_ms,
            tool_call_count,
            upstream_request_id
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(log.protocol_in)
    .bind(log.protocol_out)
    .bind(log.protocol_upstream)
    .bind(log.provider_id)
    .bind(log.provider_name)
    .bind(log.model_requested)
    .bind(log.model_upstream)
    .bind(log.status)
    .bind(log.http_status)
    .bind(log.is_streaming)
    .bind(log.input_tokens)
    .bind(log.output_tokens)
    .bind(log.reasoning_tokens)
    .bind(cost.as_ref().map(|cost| cost.estimated_cost))
    .bind(cost.as_ref().map(|cost| cost.currency.as_str()))
    .bind(log.latency_ms)
    .bind(log.upstream_latency_ms)
    .bind(log.first_token_ms)
    .bind(log.tool_call_count)
    .bind(log.upstream_request_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
struct EstimatedCost {
    estimated_cost: f64,
    currency: String,
}

fn estimate_cost(log: &RequestLogInsert, prices: &[ModelPrice]) -> Option<EstimatedCost> {
    let price = prices.iter().find(|price| {
        price.provider == log.provider_name
            && (price.model == log.model_upstream || price.model == log.model_requested)
    })?;
    let input_cost = price
        .input_price_per_1m
        .zip(log.input_tokens)
        .map(|(price, tokens)| tokens as f64 / 1_000_000.0 * price)
        .unwrap_or(0.0);
    let output_cost = price
        .output_price_per_1m
        .zip(log.output_tokens)
        .map(|(price, tokens)| tokens as f64 / 1_000_000.0 * price)
        .unwrap_or(0.0);

    Some(EstimatedCost {
        estimated_cost: input_cost + output_cost,
        currency: price.currency.clone(),
    })
}

fn load_model_prices() -> Result<Vec<ModelPrice>> {
    let path =
        std::env::var("MODEL_PRICES_PATH").unwrap_or_else(|_| "data/model_prices.json".to_string());
    if !std::path::Path::new(&path).exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn overview(pool: &SqlitePool) -> Result<StatsOverview> {
    let overview = sqlx::query_as::<_, StatsOverview>(
        r#"
        SELECT
            COUNT(*) AS total_requests,
            COALESCE(SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END), 0) AS successful_requests,
            COALESCE(SUM(CASE WHEN status <> 'success' THEN 1 ELSE 0 END), 0) AS failed_requests,
            COALESCE(SUM(input_tokens), 0) AS input_tokens,
            COALESCE(SUM(output_tokens), 0) AS output_tokens
        FROM request_logs
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(overview)
}

pub async fn list_requests(pool: &SqlitePool, limit: usize) -> Result<Vec<RequestLogSummary>> {
    let rows = sqlx::query_as::<_, RequestLogSummary>(
        r#"
        SELECT
            id,
            created_at,
            protocol_in,
            protocol_upstream,
            provider_name,
            model_requested,
            status,
            http_status,
            input_tokens,
            output_tokens,
            latency_ms
        FROM request_logs
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn list_model_stats(pool: &SqlitePool) -> Result<Vec<ModelStatsSummary>> {
    let rows = sqlx::query_as::<_, ModelStatsSummary>(
        r#"
        SELECT
            model_requested,
            COUNT(*) AS total_requests,
            COALESCE(SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END), 0) AS successful_requests,
            COALESCE(SUM(CASE WHEN status <> 'success' THEN 1 ELSE 0 END), 0) AS failed_requests,
            COALESCE(SUM(input_tokens), 0) AS input_tokens,
            COALESCE(SUM(output_tokens), 0) AS output_tokens,
            SUM(estimated_cost) AS estimated_cost,
            AVG(latency_ms) AS average_latency_ms
        FROM request_logs
        GROUP BY model_requested
        ORDER BY total_requests DESC, model_requested ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn list_provider_stats(pool: &SqlitePool) -> Result<Vec<ProviderStatsSummary>> {
    let rows = sqlx::query_as::<_, ProviderStatsSummary>(
        r#"
        SELECT
            provider_id,
            provider_name,
            COUNT(*) AS total_requests,
            COALESCE(SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END), 0) AS successful_requests,
            COALESCE(SUM(CASE WHEN status <> 'success' THEN 1 ELSE 0 END), 0) AS failed_requests,
            COALESCE(SUM(input_tokens), 0) AS input_tokens,
            COALESCE(SUM(output_tokens), 0) AS output_tokens,
            SUM(estimated_cost) AS estimated_cost,
            AVG(latency_ms) AS average_latency_ms,
            AVG(first_token_ms) AS average_first_token_ms
        FROM request_logs
        GROUP BY provider_id, provider_name
        ORDER BY total_requests DESC, provider_name ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::{
        estimate_cost, insert_request_log_with_prices, list_model_stats, list_provider_stats,
        overview, ModelPrice, RequestLogInsert,
    };
    use crate::db;

    #[tokio::test]
    async fn overview_counts_request_logs_after_schema_initialization() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                id,
                created_at,
                protocol_in,
                protocol_out,
                protocol_upstream,
                provider_id,
                provider_name,
                model_requested,
                model_upstream,
                proxy_token_id,
                status,
                http_status,
                error_code,
                error_message,
                is_streaming,
                input_tokens,
                output_tokens,
                reasoning_tokens,
                cache_read_tokens,
                cache_write_tokens,
                estimated_cost,
                currency,
                latency_ms,
                upstream_latency_ms,
                first_token_ms,
                tool_call_count,
                upstream_request_id,
                metadata_json
            )
            VALUES
                ('log-success', '2026-06-05T00:00:00Z', 'openai', 'openai', 'openai',
                 'provider-1', 'Provider One', 'gpt-4o-mini', 'gpt-4o-mini',
                 'token-1', 'success', 200, NULL, NULL, 0, 12, 34, 2, 3, 4,
                 0.000001, 'USD', 120, 100, 50, 1, 'upstream-1', '{"ok":true}'),
                ('log-failure', '2026-06-05T00:01:00Z', 'openai', 'openai', 'openai',
                 'provider-1', 'Provider One', 'gpt-4o-mini', 'gpt-4o-mini',
                 'token-1', 'failed', 500, 'upstream_error', 'upstream failed', 1,
                 5, 0, NULL, NULL, NULL, NULL, NULL, 300, 250, NULL, 0,
                 'upstream-2', NULL)
            "#,
        )
        .execute(&db)
        .await
        .expect("insert request logs");

        let overview = overview(&db).await.expect("load overview");

        assert_eq!(overview.total_requests, 2);
        assert_eq!(overview.successful_requests, 1);
        assert_eq!(overview.failed_requests, 1);
        assert_eq!(overview.input_tokens, 17);
        assert_eq!(overview.output_tokens, 34);
    }

    #[tokio::test]
    async fn model_stats_groups_request_logs_by_requested_model() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        insert_aggregate_request_logs(&db).await;

        let rows = list_model_stats(&db).await.expect("load model stats");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].model_requested.as_deref(), Some("deepseek-chat"));
        assert_eq!(rows[0].total_requests, 2);
        assert_eq!(rows[0].successful_requests, 1);
        assert_eq!(rows[0].failed_requests, 1);
        assert_eq!(rows[0].input_tokens, 17);
        assert_eq!(rows[0].output_tokens, 5);
        assert_eq!(rows[0].estimated_cost, Some(0.000012));
        assert_eq!(rows[0].average_latency_ms, Some(150.0));
        assert_eq!(rows[1].model_requested.as_deref(), Some("kimi-k2"));
    }

    #[test]
    fn estimates_cost_from_matching_model_price() {
        let cost = estimate_cost(
            &RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: "provider-1".to_string(),
                provider_name: "DeepSeek".to_string(),
                model_requested: "deepseek-chat".to_string(),
                model_upstream: "deepseek-chat".to_string(),
                status: "success".to_string(),
                http_status: 200,
                is_streaming: false,
                input_tokens: Some(1_000_000),
                output_tokens: Some(500_000),
                reasoning_tokens: None,
                latency_ms: 120,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
            &[ModelPrice {
                provider: "DeepSeek".to_string(),
                model: "deepseek-chat".to_string(),
                input_price_per_1m: Some(1.0),
                output_price_per_1m: Some(2.0),
                currency: "USD".to_string(),
            }],
        )
        .expect("estimate cost");

        assert_eq!(cost.estimated_cost, 2.0);
        assert_eq!(cost.currency, "USD");
    }

    #[test]
    fn leaves_cost_empty_when_price_is_unknown() {
        let cost = estimate_cost(
            &RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: "provider-1".to_string(),
                provider_name: "DeepSeek".to_string(),
                model_requested: "deepseek-chat".to_string(),
                model_upstream: "deepseek-chat".to_string(),
                status: "success".to_string(),
                http_status: 200,
                is_streaming: false,
                input_tokens: Some(1_000_000),
                output_tokens: Some(500_000),
                reasoning_tokens: None,
                latency_ms: 120,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
            &[],
        );

        assert!(cost.is_none());
    }

    #[tokio::test]
    async fn request_log_insert_writes_estimated_cost_when_price_matches() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        insert_request_log_with_prices(
            &db,
            RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: "provider-1".to_string(),
                provider_name: "DeepSeek".to_string(),
                model_requested: "deepseek-chat".to_string(),
                model_upstream: "deepseek-chat".to_string(),
                status: "success".to_string(),
                http_status: 200,
                is_streaming: false,
                input_tokens: Some(1_000_000),
                output_tokens: Some(500_000),
                reasoning_tokens: None,
                latency_ms: 120,
                upstream_latency_ms: None,
                first_token_ms: None,
                tool_call_count: None,
                upstream_request_id: None,
            },
            &[ModelPrice {
                provider: "DeepSeek".to_string(),
                model: "deepseek-chat".to_string(),
                input_price_per_1m: Some(1.0),
                output_price_per_1m: Some(2.0),
                currency: "USD".to_string(),
            }],
        )
        .await
        .expect("insert log");

        let row: (Option<f64>, Option<String>) =
            sqlx::query_as("SELECT estimated_cost, currency FROM request_logs LIMIT 1")
                .fetch_one(&db)
                .await
                .expect("load request log");

        assert_eq!(row.0, Some(2.0));
        assert_eq!(row.1.as_deref(), Some("USD"));
    }

    #[tokio::test]
    async fn request_log_insert_writes_observability_fields() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        insert_request_log_with_prices(
            &db,
            RequestLogInsert {
                protocol_in: "responses".to_string(),
                protocol_out: "responses".to_string(),
                protocol_upstream: "chat_completions".to_string(),
                provider_id: "provider-1".to_string(),
                provider_name: "DeepSeek".to_string(),
                model_requested: "deepseek-chat".to_string(),
                model_upstream: "deepseek-chat".to_string(),
                status: "success".to_string(),
                http_status: 200,
                is_streaming: false,
                input_tokens: Some(10),
                output_tokens: Some(5),
                reasoning_tokens: Some(2),
                latency_ms: 120,
                upstream_latency_ms: Some(90),
                first_token_ms: Some(30),
                tool_call_count: Some(1),
                upstream_request_id: Some("req_upstream".to_string()),
            },
            &[],
        )
        .await
        .expect("insert log");

        #[derive(sqlx::FromRow)]
        struct ObservabilityRow {
            reasoning_tokens: Option<i64>,
            upstream_latency_ms: Option<i64>,
            first_token_ms: Option<i64>,
            tool_call_count: Option<i64>,
            upstream_request_id: Option<String>,
        }

        let row: ObservabilityRow = sqlx::query_as(
            r#"
                SELECT
                    reasoning_tokens,
                    upstream_latency_ms,
                    first_token_ms,
                    tool_call_count,
                    upstream_request_id
                FROM request_logs
                LIMIT 1
                "#,
        )
        .fetch_one(&db)
        .await
        .expect("load request log");

        assert_eq!(row.reasoning_tokens, Some(2));
        assert_eq!(row.upstream_latency_ms, Some(90));
        assert_eq!(row.first_token_ms, Some(30));
        assert_eq!(row.tool_call_count, Some(1));
        assert_eq!(row.upstream_request_id.as_deref(), Some("req_upstream"));
    }

    #[tokio::test]
    async fn provider_stats_groups_request_logs_by_provider() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");

        insert_aggregate_request_logs(&db).await;

        let rows = list_provider_stats(&db).await.expect("load provider stats");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].provider_id.as_deref(), Some("provider-1"));
        assert_eq!(rows[0].provider_name.as_deref(), Some("Provider One"));
        assert_eq!(rows[0].total_requests, 2);
        assert_eq!(rows[0].successful_requests, 1);
        assert_eq!(rows[0].failed_requests, 1);
        assert_eq!(rows[0].input_tokens, 17);
        assert_eq!(rows[0].output_tokens, 5);
        assert_eq!(rows[0].estimated_cost, Some(0.000012));
        assert_eq!(rows[0].average_latency_ms, Some(150.0));
        assert_eq!(rows[0].average_first_token_ms, Some(50.0));
        assert_eq!(rows[1].provider_id.as_deref(), Some("provider-2"));
    }

    async fn insert_aggregate_request_logs(db: &sqlx::SqlitePool) {
        sqlx::query(
            r#"
            INSERT INTO request_logs (
                id,
                created_at,
                provider_id,
                provider_name,
                model_requested,
                status,
                input_tokens,
                output_tokens,
                estimated_cost,
                latency_ms,
                first_token_ms
            )
            VALUES
                ('log-1', '2026-06-05T00:00:00Z', 'provider-1', 'Provider One',
                 'deepseek-chat', 'success', 12, 5, 0.000012, 100, 50),
                ('log-2', '2026-06-05T00:01:00Z', 'provider-1', 'Provider One',
                 'deepseek-chat', 'failed', 5, 0, NULL, 200, NULL),
                ('log-3', '2026-06-05T00:02:00Z', 'provider-2', 'Provider Two',
                 'kimi-k2', 'success', 7, 9, 0.000034, 300, 120)
            "#,
        )
        .execute(db)
        .await
        .expect("insert aggregate request logs");
    }
}
