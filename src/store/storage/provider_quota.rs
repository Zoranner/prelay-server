use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, EntityTrait, ExprTrait, FromQueryResult, QueryFilter, QuerySelect, Select,
};

use crate::store::entity::identity::{
    activities as identity_activities, provider_shares as identity_provider_shares,
};

use super::{
    provider_usage::{integer_sum, token_total_expr},
    Storage, StorageError,
};

impl Storage {
    /// 读取某个共享用户实际生效的额度规则与起算时间。
    pub(crate) async fn shared_quota_rule(
        &self,
        provider_id: &str,
        grantee_identity_id: &str,
    ) -> Result<Option<(String, String)>, StorageError> {
        use crate::store::entity::identity::provider_configs as identity_provider_configs;

        let Some(provider) = identity_provider_configs::Entity::find_by_id(provider_id)
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let Some(json) = provider.share_quota_json else {
            return Ok(None);
        };
        let since = self
            .provider_share(provider_id, grantee_identity_id)
            .await?
            .map(|share| share.created_at)
            .unwrap_or(provider.created_at);
        Ok(Some((json, since)))
    }

    pub(crate) async fn provider_share(
        &self,
        provider_id: &str,
        grantee_identity_id: &str,
    ) -> Result<Option<identity_provider_shares::Model>, StorageError> {
        Ok(identity_provider_shares::Entity::find()
            .filter(identity_provider_shares::Column::ProviderId.eq(provider_id))
            .filter(identity_provider_shares::Column::GranteeIdentityId.eq(grantee_identity_id))
            .one(&self.db)
            .await?)
    }

    pub(crate) async fn shared_quota_used_tokens(
        &self,
        provider_id: &str,
        grantee_identity_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<i64, StorageError> {
        used_tokens(&self.db, provider_id, grantee_identity_id, start, end).await
    }
}

async fn used_tokens(
    db: &sea_orm::DatabaseConnection,
    provider_id: &str,
    grantee_identity_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<i64, StorageError> {
    let row = usage_query(provider_id, grantee_identity_id)
        .filter(identity_activities::Column::CreatedAt.gte(start.to_rfc3339()))
        .filter(identity_activities::Column::CreatedAt.lt(end.to_rfc3339()))
        .select_only()
        .column_as(integer_sum(token_total_expr().sum()), "total_tokens")
        .into_model::<UsageTotal>()
        .one(db)
        .await?;
    Ok(row.and_then(|row| row.total_tokens).unwrap_or_default())
}

fn usage_query(
    provider_id: &str,
    grantee_identity_id: &str,
) -> Select<identity_activities::Entity> {
    identity_activities::Entity::find()
        .filter(identity_activities::Column::ProviderId.eq(provider_id))
        .filter(identity_activities::Column::IdentityId.eq(grantee_identity_id))
}

#[derive(FromQueryResult)]
struct UsageTotal {
    total_tokens: Option<i64>,
}
