use axum::{
    extract::{Extension, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use prelay_protocol::{
    ModelStatsSummary, ProviderStatsSummary, RequestLogSummary, StatsOverview,
    TokenUsageTimelinePoint,
};
use serde::Deserialize;
use sqlx::FromRow;

use crate::{error::AppError, AppState};

use super::auth::CurrentIdentity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stats/overview", get(overview))
        .route("/stats/timeline", get(timeline))
        .route("/stats/requests", get(requests))
        .route("/stats/models", get(models))
        .route("/stats/providers", get(providers))
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StatsRange {
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
    ThisYear,
    LastYear,
    All,
}

#[derive(Deserialize)]
struct StatsQuery {
    range: Option<StatsRange>,
}

impl StatsQuery {
    fn range(&self) -> StatsRange {
        self.range.unwrap_or(StatsRange::Today)
    }
}

#[derive(Clone)]
struct TimeBounds {
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
}

impl TimeBounds {
    fn start_sql(&self) -> String {
        self.start.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    fn end_sql(&self) -> String {
        self.end.format("%Y-%m-%d %H:%M:%S").to_string()
    }
}

#[derive(Clone, Copy)]
enum TimelineGranularity {
    Hour,
    Day,
    Month,
}

impl StatsRange {
    fn bounds(self, now: DateTime<FixedOffset>) -> Option<TimeBounds> {
        let today = now.date_naive();
        let (start, end) = match self {
            Self::Today => (today, today + Duration::days(1)),
            Self::Yesterday => (today - Duration::days(1), today),
            Self::ThisWeek => {
                let start = today - Duration::days(today.weekday().num_days_from_monday().into());
                (start, start + Duration::days(7))
            }
            Self::LastWeek => {
                let end = today - Duration::days(today.weekday().num_days_from_monday().into());
                (end - Duration::days(7), end)
            }
            Self::ThisMonth => {
                let start = first_day_of_month(today);
                (start, first_day_of_next_month(start))
            }
            Self::LastMonth => {
                let end = first_day_of_month(today);
                (first_day_of_previous_month(end), end)
            }
            Self::ThisYear => (
                NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("valid year start"),
                NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).expect("valid next year start"),
            ),
            Self::LastYear => (
                NaiveDate::from_ymd_opt(today.year() - 1, 1, 1).expect("valid previous year start"),
                NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("valid year start"),
            ),
            Self::All => return None,
        };
        Some(TimeBounds {
            start: start_of_day(start),
            end: start_of_day(end),
        })
    }

    fn timeline_granularity(self) -> TimelineGranularity {
        match self {
            Self::Today | Self::Yesterday => TimelineGranularity::Hour,
            Self::ThisWeek | Self::LastWeek | Self::ThisMonth | Self::LastMonth => {
                TimelineGranularity::Day
            }
            Self::ThisYear | Self::LastYear | Self::All => TimelineGranularity::Month,
        }
    }
}

fn beijing_now() -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&FixedOffset::east_opt(8 * 60 * 60).expect("valid Beijing UTC offset"))
}

fn start_of_day(date: NaiveDate) -> DateTime<FixedOffset> {
    FixedOffset::east_opt(8 * 60 * 60)
        .expect("valid Beijing UTC offset")
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("valid midnight"))
        .single()
        .expect("fixed offset has no ambiguous local times")
}

fn first_day_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("every month has a first day")
}

fn first_day_of_next_month(date: NaiveDate) -> NaiveDate {
    if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1).expect("valid next January")
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1).expect("valid next month")
    }
}

fn first_day_of_previous_month(date: NaiveDate) -> NaiveDate {
    if date.month() == 1 {
        NaiveDate::from_ymd_opt(date.year() - 1, 12, 1).expect("valid previous December")
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() - 1, 1).expect("valid previous month")
    }
}

fn range_clause(has_bounds: bool) -> &'static str {
    if has_bounds {
        " AND datetime(created_at, '+8 hours') >= ? AND datetime(created_at, '+8 hours') < ?"
    } else {
        ""
    }
}

fn total_input_tokens_sum_sql(alias: &str) -> String {
    let prefix = if alias.is_empty() {
        String::new()
    } else {
        format!("{alias}.")
    };
    format!(
        "COALESCE(SUM(CASE WHEN {prefix}protocol_upstream IN ('responses', 'chat_completions') \
         AND COALESCE({prefix}input_tokens, 0) >= COALESCE({prefix}cache_read_tokens, 0) \
         THEN COALESCE({prefix}input_tokens, 0) \
         ELSE COALESCE({prefix}input_tokens, 0) + COALESCE({prefix}cache_read_tokens, 0) END \
         + COALESCE({prefix}cache_write_tokens, 0)), 0) AS total_input_tokens"
    )
}

async fn timeline(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<Vec<TokenUsageTimelinePoint>>, AppError> {
    let range = query.range();
    let bounds = match range.bounds(beijing_now()) {
        Some(bounds) => bounds,
        None => {
            let start: Option<String> = sqlx::query_scalar(
                "SELECT strftime('%Y-%m-01 00:00:00', MIN(datetime(created_at, '+8 hours'))) \
                 FROM identity_request_logs WHERE identity_id = ?",
            )
            .bind(&identity.id)
            .fetch_one(&state.db)
            .await?;
            let Some(start) = start else {
                return Ok(Json(Vec::new()));
            };
            TimeBounds {
                start: FixedOffset::east_opt(8 * 60 * 60)
                    .expect("valid Beijing UTC offset")
                    .from_local_datetime(
                        &NaiveDate::parse_from_str(&start[..10], "%Y-%m-%d")
                            .expect("database returned a valid month")
                            .and_hms_opt(0, 0, 0)
                            .expect("valid midnight"),
                    )
                    .single()
                    .expect("fixed offset has no ambiguous local times"),
                end: start_of_day(first_day_of_next_month(first_day_of_month(
                    beijing_now().date_naive(),
                ))),
            }
        }
    };
    let start = bounds.start_sql();
    let end = bounds.end_sql();
    let total_input_tokens = total_input_tokens_sum_sql("log");
    let sql = match range.timeline_granularity() {
        TimelineGranularity::Hour => {
            format!("WITH RECURSIVE buckets(bucket) AS ( \
                 SELECT ? \
                 UNION ALL \
                 SELECT datetime(bucket, '+1 hour') FROM buckets WHERE datetime(bucket, '+1 hour') < ? \
             ) \
             SELECT buckets.bucket, COALESCE(SUM(log.input_tokens), 0) AS input_tokens, \
                 {total_input_tokens}, \
                 COALESCE(SUM(log.output_tokens), 0) AS output_tokens, \
                 COALESCE(SUM(log.cache_read_tokens), 0) AS cache_read_tokens, \
                 COALESCE(SUM(log.cache_write_tokens), 0) AS cache_write_tokens \
             FROM buckets LEFT JOIN identity_request_logs AS log \
                 ON log.identity_id = ? AND datetime(log.created_at, '+8 hours') >= buckets.bucket \
                 AND datetime(log.created_at, '+8 hours') < datetime(buckets.bucket, '+1 hour') \
             GROUP BY buckets.bucket ORDER BY buckets.bucket")
        }
        TimelineGranularity::Day => {
            format!(
                "WITH RECURSIVE buckets(bucket) AS ( \
                 SELECT date(?) \
                 UNION ALL \
                 SELECT date(bucket, '+1 day') FROM buckets WHERE date(bucket, '+1 day') < date(?) \
             ) \
             SELECT buckets.bucket, COALESCE(SUM(log.input_tokens), 0) AS input_tokens, \
                 {total_input_tokens}, \
                 COALESCE(SUM(log.output_tokens), 0) AS output_tokens, \
                 COALESCE(SUM(log.cache_read_tokens), 0) AS cache_read_tokens, \
                 COALESCE(SUM(log.cache_write_tokens), 0) AS cache_write_tokens \
             FROM buckets LEFT JOIN identity_request_logs AS log \
                 ON log.identity_id = ? AND datetime(log.created_at, '+8 hours') >= buckets.bucket \
                 AND datetime(log.created_at, '+8 hours') < datetime(buckets.bucket, '+1 day') \
             GROUP BY buckets.bucket ORDER BY buckets.bucket"
            )
        }
        TimelineGranularity::Month => {
            format!("WITH RECURSIVE buckets(bucket) AS ( \
                 SELECT strftime('%Y-%m-01', ?) \
                 UNION ALL \
                 SELECT date(bucket, '+1 month') FROM buckets WHERE date(bucket, '+1 month') < date(?) \
             ) \
             SELECT buckets.bucket, COALESCE(SUM(log.input_tokens), 0) AS input_tokens, \
                 {total_input_tokens}, \
                 COALESCE(SUM(log.output_tokens), 0) AS output_tokens, \
                 COALESCE(SUM(log.cache_read_tokens), 0) AS cache_read_tokens, \
                 COALESCE(SUM(log.cache_write_tokens), 0) AS cache_write_tokens \
             FROM buckets LEFT JOIN identity_request_logs AS log \
                 ON log.identity_id = ? AND datetime(log.created_at, '+8 hours') >= buckets.bucket \
                 AND datetime(log.created_at, '+8 hours') < date(buckets.bucket, '+1 month') \
             GROUP BY buckets.bucket ORDER BY buckets.bucket")
        }
    };
    let rows = sqlx::query_as::<_, TokenUsageTimelineRow>(&sql)
        .bind(start)
        .bind(end)
        .bind(identity.id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn overview(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<StatsOverview>, AppError> {
    let bounds = query.range().bounds(beijing_now());
    let total_input_tokens = total_input_tokens_sum_sql("");
    let sql = format!(
        "SELECT COUNT(*) AS total_requests, \
         COALESCE(SUM(status = 'success'), 0) AS successful_requests, \
         COALESCE(SUM(status != 'success'), 0) AS failed_requests, \
         COALESCE(SUM(input_tokens), 0) AS input_tokens, \
         {total_input_tokens}, \
         COALESCE(SUM(output_tokens), 0) AS output_tokens, \
         COALESCE(SUM(cache_read_tokens), 0) AS cache_read_tokens, \
         COALESCE(SUM(cache_write_tokens), 0) AS cache_write_tokens, \
         CAST(AVG(latency_ms) AS INTEGER) AS average_latency_ms \
         FROM identity_request_logs WHERE identity_id = ?{}",
        range_clause(bounds.is_some())
    );
    let mut request = sqlx::query_as::<_, StatsOverviewRow>(&sql).bind(identity.id);
    if let Some(bounds) = bounds {
        request = request.bind(bounds.start_sql()).bind(bounds.end_sql());
    }
    let row = request.fetch_one(&state.db).await?;
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
        "SELECT id, created_at, protocol_in, protocol_upstream, endpoint_name, provider_name, model_requested, \
         status, http_status, error_code, error_message, input_tokens, output_tokens, is_streaming, first_token_ms, cache_read_tokens, cache_write_tokens, latency_ms, \
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
    Query(query): Query<StatsQuery>,
) -> Result<Json<Vec<ModelStatsSummary>>, AppError> {
    let bounds = query.range().bounds(beijing_now());
    let sql = format!(
        "SELECT model_requested, COUNT(*) AS total_requests, \
         COALESCE(SUM(status = 'success'), 0) AS successful_requests, \
         COALESCE(SUM(status != 'success'), 0) AS failed_requests, \
         COALESCE(SUM(input_tokens), 0) AS input_tokens, COALESCE(SUM(output_tokens), 0) AS output_tokens, \
         SUM(estimated_cost) AS estimated_cost, AVG(latency_ms) AS average_latency_ms \
         FROM identity_request_logs WHERE identity_id = ?{} GROUP BY model_requested \
         ORDER BY total_requests DESC",
        range_clause(bounds.is_some())
    );
    let mut request = sqlx::query_as::<_, ModelStatsSummaryRow>(&sql).bind(identity.id);
    if let Some(bounds) = bounds {
        request = request.bind(bounds.start_sql()).bind(bounds.end_sql());
    }
    let rows = request.fetch_all(&state.db).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn providers(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<Vec<ProviderStatsSummary>>, AppError> {
    let bounds = query.range().bounds(beijing_now());
    let sql = format!(
        "SELECT provider_id, provider_name, COUNT(*) AS total_requests, \
         COALESCE(SUM(status = 'success'), 0) AS successful_requests, \
         COALESCE(SUM(status != 'success'), 0) AS failed_requests, \
         COALESCE(SUM(input_tokens), 0) AS input_tokens, COALESCE(SUM(output_tokens), 0) AS output_tokens, \
         SUM(estimated_cost) AS estimated_cost, AVG(latency_ms) AS average_latency_ms, \
         AVG(first_token_ms) AS average_first_token_ms \
         FROM identity_request_logs WHERE identity_id = ?{} GROUP BY provider_id, provider_name \
         ORDER BY total_requests DESC",
        range_clause(bounds.is_some())
    );
    let mut request = sqlx::query_as::<_, ProviderStatsSummaryRow>(&sql).bind(identity.id);
    if let Some(bounds) = bounds {
        request = request.bind(bounds.start_sql()).bind(bounds.end_sql());
    }
    let rows = request.fetch_all(&state.db).await?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

#[derive(FromRow)]
struct StatsOverviewRow {
    total_requests: i64,
    successful_requests: i64,
    failed_requests: i64,
    input_tokens: i64,
    total_input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    average_latency_ms: Option<i64>,
}

#[derive(FromRow)]
struct TokenUsageTimelineRow {
    bucket: String,
    input_tokens: i64,
    total_input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

#[derive(FromRow)]
struct RequestLogSummaryRow {
    id: String,
    created_at: String,
    protocol_in: Option<String>,
    protocol_upstream: Option<String>,
    endpoint_name: Option<String>,
    provider_name: Option<String>,
    model_requested: Option<String>,
    status: String,
    http_status: Option<i64>,
    error_code: Option<String>,
    error_message: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    is_streaming: Option<bool>,
    first_token_ms: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
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
            total_input_tokens: value.total_input_tokens,
            output_tokens: value.output_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
            average_latency_ms: value.average_latency_ms,
        }
    }
}

impl From<TokenUsageTimelineRow> for TokenUsageTimelinePoint {
    fn from(value: TokenUsageTimelineRow) -> Self {
        Self {
            bucket: value.bucket,
            input_tokens: value.input_tokens,
            total_input_tokens: value.total_input_tokens,
            output_tokens: value.output_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
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
            endpoint_name: value.endpoint_name,
            provider_name: value.provider_name,
            model_requested: value.model_requested,
            status: value.status,
            http_status: value.http_status,
            error_code: value.error_code,
            error_message: value.error_message,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            is_streaming: value.is_streaming,
            first_token_ms: value.first_token_ms,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
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
