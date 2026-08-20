use axum::{
    extract::{Extension, Query, State},
    routing::get,
    Json, Router,
};
use prelay_protocol::{ModelStatsSummary, ProviderStatsSummary, RequestLogSummary, StatsOverview};
use serde::Deserialize;
use sqlx::FromRow;

use crate::{error::AppError, AppState};

use super::auth::CurrentIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stats/overview", get(overview))
        .route("/stats/requests", get(requests))
        .route("/stats/models", get(models))
        .route("/stats/providers", get(providers))
}

async fn overview(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<StatsOverview>, AppError> {
    let row = sqlx::query_as::<_, StatsOverviewRow>(
        "SELECT COUNT(*) AS total_requests, \
         COALESCE(SUM(status = 'success'), 0) AS successful_requests, \
         COALESCE(SUM(status != 'success'), 0) AS failed_requests, \
         COALESCE(SUM(input_tokens), 0) AS input_tokens, \
         COALESCE(SUM(output_tokens), 0) AS output_tokens \
         FROM identity_request_logs WHERE identity_id = ?",
    )
    .bind(identity.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(row.into()))
}

#[derive(Deserialize)]
struct RequestQuery {
    limit: Option<usize>,
}

async fn requests(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Query(query): Query<RequestQuery>,
) -> Result<Json<Vec<RequestLogSummary>>, AppError> {
    let limit = query.limit.unwrap_or(100).min(500) as i64;
    let rows = sqlx::query_as::<_, RequestLogSummaryRow>(
        "SELECT id, created_at, protocol_in, protocol_upstream, provider_name, model_requested, \
         status, http_status, error_code, error_message, input_tokens, output_tokens, latency_ms, \
         upstream_request_id, metadata_json FROM identity_request_logs \
         WHERE identity_id = ? ORDER BY created_at DESC LIMIT ?",
    )
    .bind(identity.id)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn models(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<Vec<ModelStatsSummary>>, AppError> {
    let rows = sqlx::query_as::<_, ModelStatsSummaryRow>(
        "SELECT model_requested, COUNT(*) AS total_requests, \
         COALESCE(SUM(status = 'success'), 0) AS successful_requests, \
         COALESCE(SUM(status != 'success'), 0) AS failed_requests, \
         COALESCE(SUM(input_tokens), 0) AS input_tokens, COALESCE(SUM(output_tokens), 0) AS output_tokens, \
         SUM(estimated_cost) AS estimated_cost, AVG(latency_ms) AS average_latency_ms \
         FROM identity_request_logs WHERE identity_id = ? GROUP BY model_requested \
         ORDER BY total_requests DESC",
    )
    .bind(identity.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn providers(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
) -> Result<Json<Vec<ProviderStatsSummary>>, AppError> {
    let rows = sqlx::query_as::<_, ProviderStatsSummaryRow>(
        "SELECT provider_id, provider_name, COUNT(*) AS total_requests, \
         COALESCE(SUM(status = 'success'), 0) AS successful_requests, \
         COALESCE(SUM(status != 'success'), 0) AS failed_requests, \
         COALESCE(SUM(input_tokens), 0) AS input_tokens, COALESCE(SUM(output_tokens), 0) AS output_tokens, \
         SUM(estimated_cost) AS estimated_cost, AVG(latency_ms) AS average_latency_ms, \
         AVG(first_token_ms) AS average_first_token_ms \
         FROM identity_request_logs WHERE identity_id = ? GROUP BY provider_id, provider_name \
         ORDER BY total_requests DESC",
    )
    .bind(identity.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

#[derive(FromRow)]
struct StatsOverviewRow {
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    input_tokens: i64,
    output_tokens: i64,
}

#[derive(FromRow)]
struct RequestLogSummaryRow {
    id: String,
    created_at: String,
    protocol_in: Option<String>,
    protocol_upstream: Option<String>,
    provider_name: Option<String>,
    model_requested: Option<String>,
    status: String,
    http_status: Option<i64>,
    error_code: Option<String>,
    error_message: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    latency_ms: Option<i64>,
    upstream_request_id: Option<String>,
    metadata_json: Option<String>,
}

#[derive(FromRow)]
struct ModelStatsSummaryRow {
    model_requested: Option<String>,
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    estimated_cost: Option<f64>,
    average_latency_ms: Option<f64>,
}

#[derive(FromRow)]
struct ProviderStatsSummaryRow {
    provider_id: Option<String>,
    provider_name: Option<String>,
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    estimated_cost: Option<f64>,
    average_latency_ms: Option<f64>,
    average_first_token_ms: Option<f64>,
}

impl From<StatsOverviewRow> for StatsOverview {
    fn from(value: StatsOverviewRow) -> Self {
        Self {
            total_requests: value.total_requests,
            successful_requests: value.successful_requests,
            failed_requests: value.failed_requests,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
        }
    }
}

impl From<RequestLogSummaryRow> for RequestLogSummary {
    fn from(value: RequestLogSummaryRow) -> Self {
        Self {
            id: value.id,
            created_at: value.created_at,
            protocol_in: value.protocol_in,
            protocol_upstream: value.protocol_upstream,
            provider_name: value.provider_name,
            model_requested: value.model_requested,
            status: value.status,
            http_status: value.http_status,
            error_code: value.error_code,
            error_message: value.error_message,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            latency_ms: value.latency_ms,
            upstream_request_id: value.upstream_request_id,
            metadata_json: value.metadata_json,
        }
    }
}

impl From<ModelStatsSummaryRow> for ModelStatsSummary {
    fn from(value: ModelStatsSummaryRow) -> Self {
        Self {
            model_requested: value.model_requested,
            total_requests: value.total_requests,
            successful_requests: value.successful_requests,
            failed_requests: value.failed_requests,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            estimated_cost: value.estimated_cost,
            average_latency_ms: value.average_latency_ms,
        }
    }
}

impl From<ProviderStatsSummaryRow> for ProviderStatsSummary {
    fn from(value: ProviderStatsSummaryRow) -> Self {
        Self {
            provider_id: value.provider_id,
            provider_name: value.provider_name,
            total_requests: value.total_requests,
            successful_requests: value.successful_requests,
            failed_requests: value.failed_requests,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            estimated_cost: value.estimated_cost,
            average_latency_ms: value.average_latency_ms,
            average_first_token_ms: value.average_first_token_ms,
        }
    }
}
