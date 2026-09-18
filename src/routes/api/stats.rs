use axum::{
    extract::{Extension, State},
    routing::get,
    Json, Router,
};
use prelay_protocol::{
    stats::{LeaderboardMetric, UserLeaderboardEntry},
    ActivitySummary, ModelStatsSummary, ProviderStatsSummary, StatsOverview,
    TokenUsageTimelinePoint,
};
use serde::Deserialize;

use crate::{
    stats::{ModelStatsScope, StatsRange},
    AppState,
};

use super::auth::CurrentIdentity;
use super::{ApiError, ApiQuery};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stats/overview", get(overview))
        .route("/stats/timeline", get(timeline))
        .route("/stats/activities", get(activities))
        .route("/stats/models", get(models))
        .route("/stats/providers", get(providers))
        .route("/stats/leaderboard", get(leaderboard))
}

#[derive(Deserialize)]
struct StatsQuery {
    range: Option<StatsRange>,
}

impl StatsQuery {
    fn range(&self) -> StatsRange {
        self.range.unwrap_or_default()
    }
}

#[derive(Deserialize)]
struct ModelStatsQuery {
    range: Option<StatsRange>,
    scope: Option<ModelStatsScope>,
}

impl ModelStatsQuery {
    fn range(&self) -> StatsRange {
        self.range.unwrap_or_default()
    }

    fn scope(&self) -> ModelStatsScope {
        self.scope.unwrap_or_default()
    }
}

/// 目前只放开按天粒度：其余粒度仍按 range 的默认映射，其它取值按非法参数拒绝。
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TimelineGranularityParam {
    Day,
}

#[derive(Deserialize)]
struct TimelineQuery {
    range: Option<StatsRange>,
    granularity: Option<TimelineGranularityParam>,
}

impl TimelineQuery {
    fn range(&self) -> StatsRange {
        self.range.unwrap_or_default()
    }
}

async fn timeline(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    ApiQuery(query): ApiQuery<TimelineQuery>,
) -> Result<Json<Vec<TokenUsageTimelinePoint>>, ApiError> {
    let points = match query.granularity {
        Some(TimelineGranularityParam::Day) => {
            state
                .storage
                .daily_token_usage_timeline(&identity.id, query.range())
                .await?
        }
        None => {
            state
                .storage
                .token_usage_timeline(&identity.id, query.range())
                .await?
        }
    };
    Ok(Json(points))
}

async fn overview(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    ApiQuery(query): ApiQuery<StatsQuery>,
) -> Result<Json<StatsOverview>, ApiError> {
    Ok(Json(
        state
            .storage
            .stats_overview(&identity.id, query.range())
            .await?,
    ))
}

#[derive(Deserialize)]
struct RequestQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct LeaderboardQuery {
    range: Option<StatsRange>,
    metric: Option<LeaderboardMetric>,
    limit: Option<usize>,
}

async fn activities(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    ApiQuery(query): ApiQuery<RequestQuery>,
) -> Result<Json<Vec<ActivitySummary>>, ApiError> {
    Ok(Json(
        state
            .storage
            .list_activities_with_catalog(
                &identity.id,
                query.limit.unwrap_or(100),
                &state.provider_catalog,
            )
            .await?,
    ))
}

async fn leaderboard(
    State(state): State<AppState>,
    Extension(_identity): Extension<CurrentIdentity>,
    ApiQuery(query): ApiQuery<LeaderboardQuery>,
) -> Result<Json<Vec<UserLeaderboardEntry>>, ApiError> {
    Ok(Json(
        state
            .storage
            .user_leaderboard(
                query.range.unwrap_or_default(),
                query.metric.unwrap_or(LeaderboardMetric::Activities),
                query.limit.unwrap_or(50),
            )
            .await?,
    ))
}

async fn models(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    ApiQuery(query): ApiQuery<ModelStatsQuery>,
) -> Result<Json<Vec<ModelStatsSummary>>, ApiError> {
    Ok(Json(
        state
            .storage
            .model_stats_with_catalog(
                &identity.id,
                query.scope(),
                query.range(),
                &state.provider_catalog,
            )
            .await?,
    ))
}

async fn providers(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    ApiQuery(query): ApiQuery<StatsQuery>,
) -> Result<Json<Vec<ProviderStatsSummary>>, ApiError> {
    Ok(Json(
        state
            .storage
            .provider_stats(&identity.id, query.range())
            .await?,
    ))
}
