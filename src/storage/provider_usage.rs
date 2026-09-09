use std::collections::HashMap;

use chrono::Utc;
use prelay_protocol::{ProviderUsageResponse, ProviderUsageUser};
use sea_orm::{
    sea_query::Expr, ColumnTrait, DatabaseConnection, EntityTrait, ExprTrait, FromQueryResult,
    QueryFilter, QuerySelect, Select,
};

use crate::{
    entity::{identities, identity::activities as identity_activities},
    stats::{StatsRange, TimeBounds},
};

use super::{Storage, StorageError};

impl Storage {
    pub async fn get_visible_provider_usage(
        &self,
        identity_id: &str,
        provider_id: &str,
        range: StatsRange,
    ) -> Result<ProviderUsageResponse, StorageError> {
        self.get_visible_provider(identity_id, provider_id).await?;
        usage(&self.db, provider_id, range).await
    }
}

async fn usage(
    db: &DatabaseConnection,
    provider_id: &str,
    range: StatsRange,
) -> Result<ProviderUsageResponse, StorageError> {
    let bounds = range.bounds(Utc::now());
    let aggregate = usage_query(provider_id, bounds)
        .select_only()
        .column_as(identity_activities::Column::Id.count(), "total_requests")
        .column_as(
            integer_sum(identity_activities::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(
            integer_sum(identity_activities::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(integer_sum(token_total_expr().sum()), "total_tokens")
        .column_as(
            identity_activities::Column::CreatedAt.max(),
            "latest_used_at",
        )
        .into_model::<UsageAggregate>()
        .one(db)
        .await?
        .unwrap_or_default();

    let rows = usage_query(provider_id, bounds)
        .select_only()
        .column(identity_activities::Column::IdentityId)
        .column_as(identity_activities::Column::Id.count(), "request_count")
        .column_as(
            integer_sum(identity_activities::Column::InputTokens.sum()),
            "input_tokens",
        )
        .column_as(
            integer_sum(identity_activities::Column::OutputTokens.sum()),
            "output_tokens",
        )
        .column_as(integer_sum(token_total_expr().sum()), "total_tokens")
        .column_as(
            identity_activities::Column::CreatedAt.max(),
            "latest_used_at",
        )
        .group_by(identity_activities::Column::IdentityId)
        .into_model::<UserUsageAggregate>()
        .all(db)
        .await?;

    let identity_ids = rows
        .iter()
        .map(|row| row.identity_id.clone())
        .collect::<Vec<_>>();
    let display_names = identities::Entity::find()
        .filter(identities::Column::Id.is_in(identity_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|identity| (identity.id, identity.display_name))
        .collect::<HashMap<_, _>>();

    let mut users = rows
        .into_iter()
        .map(|row| {
            let display_name = display_names
                .get(&row.identity_id)
                .cloned()
                .ok_or(StorageError::IdentityNotFound)?;
            Ok(ProviderUsageUser {
                identity_id: row.identity_id,
                display_name,
                request_count: row.request_count,
                input_tokens: row.input_tokens.unwrap_or_default(),
                output_tokens: row.output_tokens.unwrap_or_default(),
                total_tokens: row.total_tokens.unwrap_or_default(),
                latest_used_at: row.latest_used_at,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    users.sort_by(|left, right| {
        right
            .request_count
            .cmp(&left.request_count)
            .then_with(|| left.identity_id.cmp(&right.identity_id))
    });

    Ok(ProviderUsageResponse {
        total_requests: aggregate.total_requests,
        input_tokens: aggregate.input_tokens.unwrap_or_default(),
        output_tokens: aggregate.output_tokens.unwrap_or_default(),
        total_tokens: aggregate.total_tokens.unwrap_or_default(),
        latest_used_at: aggregate.latest_used_at,
        users,
    })
}

fn usage_query(
    provider_id: &str,
    bounds: Option<TimeBounds>,
) -> Select<identity_activities::Entity> {
    let mut query = identity_activities::Entity::find()
        .filter(identity_activities::Column::ProviderId.eq(provider_id))
        .filter(identity_activities::Column::IdentityId.is_not_null());
    if let Some(bounds) = bounds {
        query = query
            .filter(identity_activities::Column::CreatedAt.gte(bounds.start.to_rfc3339()))
            .filter(identity_activities::Column::CreatedAt.lt(bounds.end.to_rfc3339()));
    }
    query
}

fn integer_sum(expr: Expr) -> Expr {
    expr.cast_as("bigint")
}

fn token_total_expr() -> Expr {
    Expr::col(identity_activities::Column::InputTokens)
        .if_null(0)
        .add(Expr::col(identity_activities::Column::OutputTokens).if_null(0))
}

#[derive(Default, FromQueryResult)]
struct UsageAggregate {
    total_requests: i64,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    latest_used_at: Option<String>,
}

#[derive(FromQueryResult)]
struct UserUsageAggregate {
    identity_id: String,
    request_count: i64,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    latest_used_at: Option<String>,
}
