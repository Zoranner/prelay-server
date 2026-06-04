use anyhow::Result;
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
    pub latency_ms: i64,
}

pub async fn insert_request_log(pool: &SqlitePool, log: RequestLogInsert) -> Result<()> {
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
            latency_ms
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
    .bind(log.latency_ms)
    .execute(pool)
    .await?;

    Ok(())
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

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::overview;
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
}
