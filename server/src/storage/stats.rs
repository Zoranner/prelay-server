use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::stats::{
    estimate_cost, load_model_prices, ModelPrice, RequestLogInsert, StreamRequestLogUpdate,
};

pub async fn insert(pool: &SqlitePool, identity_id: &str, log: RequestLogInsert) -> Result<()> {
    insert_with_id(pool, identity_id, Uuid::new_v4().to_string(), log).await
}

pub async fn insert_with_id(
    pool: &SqlitePool,
    identity_id: &str,
    id: String,
    log: RequestLogInsert,
) -> Result<()> {
    let prices = load_model_prices().unwrap_or_default();
    insert_with_id_and_prices(pool, identity_id, id, log, &prices).await
}

async fn insert_with_id_and_prices(
    pool: &SqlitePool,
    identity_id: &str,
    id: String,
    log: RequestLogInsert,
    prices: &[ModelPrice],
) -> Result<()> {
    let cost = estimate_cost(&log, prices);
    sqlx::query(
        "INSERT INTO identity_request_logs (id, identity_id, created_at, protocol_in, protocol_out, \
         protocol_upstream, provider_id, provider_name, model_requested, model_upstream, status, \
         http_status, error_code, error_message, is_streaming, input_tokens, output_tokens, \
         reasoning_tokens, estimated_cost, currency, latency_ms, upstream_latency_ms, first_token_ms, \
         tool_call_count, upstream_request_id, metadata_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
         ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(identity_id)
    .bind(Utc::now().to_rfc3339())
    .bind(log.protocol_in)
    .bind(log.protocol_out)
    .bind(log.protocol_upstream)
    .bind(log.provider_id)
    .bind(log.provider_name)
    .bind(log.model_requested)
    .bind(log.model_upstream)
    .bind(log.status)
    .bind(log.http_status)
    .bind(log.error_code)
    .bind(log.error_message)
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
    .bind(log.metadata_json)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_stream(
    pool: &SqlitePool,
    identity_id: &str,
    id: &str,
    update: StreamRequestLogUpdate,
) -> Result<()> {
    let prices = load_model_prices().unwrap_or_default();
    update_stream_with_prices(pool, identity_id, id, update, &prices).await
}

async fn update_stream_with_prices(
    pool: &SqlitePool,
    identity_id: &str,
    id: &str,
    update: StreamRequestLogUpdate,
    prices: &[ModelPrice],
) -> Result<()> {
    let cost = estimate_cost_for_existing_log(pool, identity_id, id, &update, prices).await?;
    sqlx::query(
        "UPDATE identity_request_logs SET status = ?, http_status = ?, error_code = ?, \
         error_message = ?, input_tokens = ?, output_tokens = ?, reasoning_tokens = ?, \
         estimated_cost = ?, currency = ?, latency_ms = ?, tool_call_count = ?, \
         upstream_request_id = COALESCE(?, upstream_request_id), \
         metadata_json = ? WHERE id = ? AND identity_id = ?",
    )
    .bind(update.status)
    .bind(update.http_status)
    .bind(update.error_code)
    .bind(update.error_message)
    .bind(update.input_tokens)
    .bind(update.output_tokens)
    .bind(update.reasoning_tokens)
    .bind(cost.as_ref().map(|cost| cost.estimated_cost))
    .bind(cost.as_ref().map(|cost| cost.currency.as_str()))
    .bind(update.latency_ms)
    .bind(update.tool_call_count)
    .bind(update.upstream_request_id)
    .bind(update.metadata_json)
    .bind(id)
    .bind(identity_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn estimate_cost_for_existing_log(
    pool: &SqlitePool,
    identity_id: &str,
    id: &str,
    update: &StreamRequestLogUpdate,
    prices: &[ModelPrice],
) -> Result<Option<crate::stats::EstimatedCost>> {
    #[derive(sqlx::FromRow)]
    struct PricingContext {
        provider_name: String,
        model_requested: String,
        model_upstream: String,
    }

    let Some(context) = sqlx::query_as::<_, PricingContext>(
        "SELECT COALESCE(provider_name, '') AS provider_name, \
         COALESCE(model_requested, '') AS model_requested, \
         COALESCE(model_upstream, '') AS model_upstream \
         FROM identity_request_logs WHERE id = ? AND identity_id = ?",
    )
    .bind(id)
    .bind(identity_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    let log = RequestLogInsert {
        protocol_in: String::new(),
        protocol_out: String::new(),
        protocol_upstream: String::new(),
        provider_id: String::new(),
        provider_name: context.provider_name,
        model_requested: context.model_requested,
        model_upstream: context.model_upstream,
        status: update.status.clone(),
        http_status: update.http_status,
        error_code: update.error_code.clone(),
        error_message: update.error_message.clone(),
        is_streaming: true,
        input_tokens: update.input_tokens,
        output_tokens: update.output_tokens,
        reasoning_tokens: update.reasoning_tokens,
        latency_ms: update.latency_ms,
        upstream_latency_ms: None,
        first_token_ms: None,
        tool_call_count: update.tool_call_count,
        upstream_request_id: update.upstream_request_id.clone(),
        metadata_json: update.metadata_json.clone(),
    };
    Ok(estimate_cost(&log, prices))
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::{insert_with_id_and_prices, update_stream_with_prices};
    use crate::{
        stats::{ModelPrice, RequestLogInsert, StreamRequestLogUpdate},
        storage::{MasterKey, Storage},
    };

    #[tokio::test]
    async fn identity_logs_persist_cost_for_regular_and_streaming_requests() {
        let storage = Storage::initialize(
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("create database"),
            MasterKey::from_bytes([0; 32]),
        )
        .await
        .expect("initialize storage");
        let identity = storage
            .register_identity("machine-1", "S-1-5-21-1", "credential-1")
            .await
            .expect("register identity");
        let pool = storage.pool();
        let prices = [ModelPrice {
            provider: "Provider One".to_string(),
            model: "model-1".to_string(),
            input_price_per_1m: Some(1.0),
            output_price_per_1m: Some(2.0),
            currency: "USD".to_string(),
        }];

        insert_with_id_and_prices(
            pool,
            &identity.identity_id,
            "regular".to_string(),
            test_log(Some(1_000_000), Some(500_000)),
            &prices,
        )
        .await
        .expect("insert regular log");
        insert_with_id_and_prices(
            pool,
            &identity.identity_id,
            "stream".to_string(),
            test_log(None, None),
            &prices,
        )
        .await
        .expect("insert stream log");
        update_stream_with_prices(
            pool,
            &identity.identity_id,
            "stream",
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
        .expect("update stream log");

        let rows: Vec<(String, Option<f64>, Option<String>)> = sqlx::query_as(
            "SELECT id, estimated_cost, currency FROM identity_request_logs ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .expect("load costs");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|(_, cost, currency)| {
            *cost == Some(2.0) && currency.as_deref() == Some("USD")
        }));
    }

    fn test_log(input_tokens: Option<i64>, output_tokens: Option<i64>) -> RequestLogInsert {
        RequestLogInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "chat_completions".to_string(),
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
            latency_ms: 10,
            upstream_latency_ms: None,
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
            metadata_json: None,
        }
    }
}
