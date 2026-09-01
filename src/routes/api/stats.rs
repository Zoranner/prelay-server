use axum::{
    extract::{Extension, Query, State},
    routing::get,
    Json, Router,
};
use prelay_protocol::{
    stats::{LeaderboardMetric, UserLeaderboardEntry},
    ActivitySummary, ModelStatsSummary, ProviderStatsSummary, StatsOverview,
    TokenUsageTimelinePoint,
};
use serde::Deserialize;

use crate::{error::AppError, stats::StatsRange, AppState};

use super::auth::CurrentIdentity;

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

async fn timeline(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<Vec<TokenUsageTimelinePoint>>, AppError> {
    Ok(Json(
        state
            .storage
            .token_usage_timeline(&identity.id, query.range())
            .await?,
    ))
}

async fn overview(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<StatsOverview>, AppError> {
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
    Query(query): Query<RequestQuery>,
) -> Result<Json<Vec<ActivitySummary>>, AppError> {
    Ok(Json(
        state
            .storage
            .list_activities(&identity.id, query.limit.unwrap_or(100))
            .await?,
    ))
}

async fn leaderboard(
    State(state): State<AppState>,
    Extension(_identity): Extension<CurrentIdentity>,
    Query(query): Query<LeaderboardQuery>,
) -> Result<Json<Vec<UserLeaderboardEntry>>, AppError> {
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
    Query(query): Query<StatsQuery>,
) -> Result<Json<Vec<ModelStatsSummary>>, AppError> {
    Ok(Json(
        state
            .storage
            .model_stats(&identity.id, query.range())
            .await?,
    ))
}

async fn providers(
    State(state): State<AppState>,
    Extension(identity): Extension<CurrentIdentity>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<Vec<ProviderStatsSummary>>, AppError> {
    Ok(Json(
        state
            .storage
            .provider_stats(&identity.id, query.range())
            .await?,
    ))
}
