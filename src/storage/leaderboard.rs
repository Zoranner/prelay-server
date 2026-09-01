use chrono::Utc;
use prelay_protocol::stats::{LeaderboardMetric, UserLeaderboardEntry};
use sea_orm::{
    sea_query::{Expr, ExprTrait},
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QuerySelect, RelationTrait,
};

use crate::{
    entity::{identities, identity::activities as identity_activities},
    stats::StatsRange,
    storage::{Storage, StorageError},
};

impl Storage {
    pub async fn user_leaderboard(
        &self,
        range: StatsRange,
        metric: LeaderboardMetric,
        limit: usize,
    ) -> Result<Vec<UserLeaderboardEntry>, StorageError> {
        user_leaderboard(&self.db, range, metric, limit).await
    }
}

async fn user_leaderboard(
    db: &DatabaseConnection,
    range: StatsRange,
    metric: LeaderboardMetric,
    limit: usize,
) -> Result<Vec<UserLeaderboardEntry>, StorageError> {
    let mut query = identities::Entity::find()
        .join(JoinType::InnerJoin, identities::Relation::Activities.def())
        .select_only()
        .column_as(identities::Column::Id, "identity_id")
        .column(identities::Column::DisplayName)
        .column_as(identity_activities::Column::Id.count(), "activity_count")
        .column_as(integer_sum(token_total_expr().sum()), "total_tokens")
        .column_as(success_count_expr(), "successful_activities")
        .group_by(identities::Column::Id)
        .group_by(identities::Column::DisplayName);
    if let Some(bounds) = range.bounds(Utc::now()) {
        query = query
            .filter(identity_activities::Column::CreatedAt.gte(bounds.start.to_rfc3339()))
            .filter(identity_activities::Column::CreatedAt.lt(bounds.end.to_rfc3339()));
    }
    let rows = query
        .into_model::<UserLeaderboardAggregate>()
        .all(db)
        .await?;
    let mut entries = rows
        .into_iter()
        .map(|row| {
            let activity_count = row.activity_count;
            let successful_activities = row.successful_activities.unwrap_or_default();
            UserLeaderboardEntry {
                rank: 0,
                identity_id: row.identity_id,
                display_name: if row.display_name.trim().is_empty() {
                    "未命名身份".to_owned()
                } else {
                    row.display_name
                },
                activity_count,
                total_tokens: row.total_tokens.unwrap_or_default(),
                successful_activities,
                success_rate: if activity_count == 0 {
                    0.0
                } else {
                    successful_activities as f64 / activity_count as f64
                },
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        let ordering = match metric {
            LeaderboardMetric::Activities => right.activity_count.cmp(&left.activity_count),
            LeaderboardMetric::TotalTokens => right.total_tokens.cmp(&left.total_tokens),
            LeaderboardMetric::SuccessfulActivities => {
                right.successful_activities.cmp(&left.successful_activities)
            }
            LeaderboardMetric::SuccessRate => right
                .success_rate
                .partial_cmp(&left.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal),
        };
        ordering.then_with(|| left.identity_id.cmp(&right.identity_id))
    });
    entries.truncate(limit.clamp(1, 100));
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.rank = index as i64 + 1;
    }
    Ok(entries)
}

fn token_total_expr() -> Expr {
    Expr::col(identity_activities::Column::InputTokens)
        .if_null(0)
        .add(Expr::col(identity_activities::Column::OutputTokens).if_null(0))
}

fn success_count_expr() -> Expr {
    let case: Expr = Expr::case(
        Expr::col(identity_activities::Column::Status).eq("success"),
        1,
    )
    .finally(0)
    .into();
    case.sum()
}

fn integer_sum(expr: Expr) -> Expr {
    expr.cast_as("bigint")
}

#[derive(FromQueryResult)]
struct UserLeaderboardAggregate {
    identity_id: String,
    display_name: String,
    activity_count: i64,
    total_tokens: Option<i64>,
    successful_activities: Option<i64>,
}
