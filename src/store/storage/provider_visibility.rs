use std::collections::{HashMap, HashSet};

use chrono::Utc;
use prelay_protocol::{
    ProviderResponse, ProviderSharingResponse, ProviderSharingUserResponse, ProviderVisibility,
    UpdateProviderSharingRequest,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, TransactionTrait,
};

use crate::domain::catalog::ProviderCatalog;
use crate::store::entity::{
    identities,
    identity::{
        provider_configs as identity_provider_configs, provider_shares as identity_provider_shares,
    },
};

use super::{provider_views::provider_response, Storage, StorageError};

mod share_quota;
pub(crate) use share_quota::parse_share_quota;
use share_quota::validate_share_quota;
mod visibility;
use visibility::{parse_visibility, visibility_value};

#[derive(Clone, Debug)]
pub(crate) struct ProviderGrant {
    pub(crate) created_at: String,
}

#[derive(Clone, Debug)]
pub(crate) struct VisibleProvider {
    pub(crate) provider: identity_provider_configs::Model,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_display_name: String,
    pub(crate) visibility: ProviderVisibility,
    pub(crate) is_owner: bool,
    pub(crate) grant: Option<ProviderGrant>,
}

impl Storage {
    pub(crate) async fn list_visible_provider_records(
        &self,
        identity_id: &str,
    ) -> Result<Vec<VisibleProvider>, StorageError> {
        let providers = identity_provider_configs::Entity::find()
            .order_by_asc(identity_provider_configs::Column::CreatedAt)
            .all(&self.db)
            .await?;
        let mut visible = Vec::new();
        for provider in providers {
            if let Some(provider) = visible_provider(&self.db, identity_id, provider).await? {
                visible.push(provider);
            }
        }
        Ok(visible)
    }

    pub async fn list_visible_providers(
        &self,
        identity_id: &str,
        catalog: Option<&ProviderCatalog>,
    ) -> Result<Vec<ProviderResponse>, StorageError> {
        let providers = identity_provider_configs::Entity::find()
            .order_by_asc(identity_provider_configs::Column::CreatedAt)
            .all(&self.db)
            .await?;
        let mut visible = Vec::new();
        for provider in providers {
            let Some(provider) = visible_provider(&self.db, identity_id, provider).await? else {
                continue;
            };
            visible.push(provider_response(&self.crypto, catalog, provider, false)?);
        }
        Ok(visible)
    }

    pub(crate) async fn get_visible_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<VisibleProvider, StorageError> {
        let provider = identity_provider_configs::Entity::find_by_id(provider_id)
            .one(&self.db)
            .await?
            .ok_or(StorageError::ProviderNotVisible)?;
        visible_provider(&self.db, identity_id, provider)
            .await?
            .ok_or(StorageError::ProviderNotVisible)
    }

    pub async fn can_use_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<bool, StorageError> {
        let Some(provider) = identity_provider_configs::Entity::find_by_id(provider_id)
            .one(&self.db)
            .await?
        else {
            return Ok(false);
        };
        Ok(visible_provider(&self.db, identity_id, provider)
            .await?
            .is_some())
    }

    pub async fn get_provider_sharing(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<ProviderSharingResponse, StorageError> {
        let provider = self.get_visible_provider(identity_id, provider_id).await?;
        if !provider.is_owner {
            return Err(StorageError::ProviderSharingNotAllowed);
        }
        Ok(ProviderSharingResponse {
            visibility: provider.visibility,
            users: provider_sharing_users(&self.db, provider_id).await?,
            quota: provider
                .provider
                .share_quota_json
                .as_deref()
                .map(parse_share_quota)
                .transpose()?,
        })
    }

    pub async fn update_provider_sharing(
        &self,
        identity_id: &str,
        provider_id: &str,
        input: UpdateProviderSharingRequest,
        catalog: &ProviderCatalog,
    ) -> Result<ProviderSharingResponse, StorageError> {
        let transaction = self.db.begin().await?;
        let provider = owned_provider(&transaction, identity_id, provider_id).await?;
        super::provider_validation::ensure_provider_shareable(catalog, &provider.provider_type)?;
        validate_sharing_input(&transaction, identity_id, input.visibility, &input).await?;

        let mut provider = provider.into_active_model();
        provider.visibility = Set(visibility_value(input.visibility).to_string());
        provider.share_quota_json = Set(input
            .quota
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| {
                StorageError::InvalidProviderSharing(format!(
                    "could not serialize sharing quota: {error}"
                ))
            })?);
        provider.update(&transaction).await?;

        let existing_created_at = identity_provider_shares::Entity::find()
            .filter(identity_provider_shares::Column::ProviderId.eq(provider_id))
            .all(&transaction)
            .await?
            .into_iter()
            .map(|share| (share.grantee_identity_id, share.created_at))
            .collect::<HashMap<_, _>>();
        identity_provider_shares::Entity::delete_many()
            .filter(identity_provider_shares::Column::ProviderId.eq(provider_id))
            .exec(&transaction)
            .await?;
        let now = Utc::now().to_rfc3339();
        for user in input.users {
            let created_at = existing_created_at
                .get(&user.identity_id)
                .cloned()
                .unwrap_or_else(|| now.clone());
            identity_provider_shares::ActiveModel {
                provider_id: Set(provider_id.to_string()),
                grantee_identity_id: Set(user.identity_id),
                created_at: Set(created_at),
                quota_json: Set(None),
            }
            .insert(&transaction)
            .await?;
        }
        transaction.commit().await?;
        self.get_provider_sharing(identity_id, provider_id).await
    }
}

pub(crate) async fn can_use_provider_on<C>(
    db: &C,
    identity_id: &str,
    provider_id: &str,
) -> Result<bool, StorageError>
where
    C: ConnectionTrait,
{
    let Some(provider) = identity_provider_configs::Entity::find_by_id(provider_id)
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    Ok(visible_provider(db, identity_id, provider).await?.is_some())
}

pub(crate) async fn owned_provider<C>(
    db: &C,
    identity_id: &str,
    provider_id: &str,
) -> Result<identity_provider_configs::Model, StorageError>
where
    C: ConnectionTrait,
{
    let provider = identity_provider_configs::Entity::find_by_id(provider_id)
        .one(db)
        .await?
        .ok_or(StorageError::ProviderNotFound)?;
    if provider.identity_id == identity_id {
        return Ok(provider);
    }
    if visible_provider(db, identity_id, provider).await?.is_some() {
        return Err(StorageError::ProviderSharingNotAllowed);
    }
    Err(StorageError::ProviderNotFound)
}

pub(crate) async fn visible_provider<C>(
    db: &C,
    identity_id: &str,
    provider: identity_provider_configs::Model,
) -> Result<Option<VisibleProvider>, StorageError>
where
    C: ConnectionTrait,
{
    let visibility = parse_visibility(&provider.visibility)?;
    let is_owner = provider.identity_id == identity_id;
    let mut grant = None;
    let visible = is_owner
        || match visibility {
            ProviderVisibility::Private => false,
            ProviderVisibility::All => identities::Entity::find_by_id(identity_id)
                .one(db)
                .await?
                .is_some(),
            ProviderVisibility::Selected => {
                grant = identity_provider_shares::Entity::find()
                    .filter(identity_provider_shares::Column::ProviderId.eq(&provider.id))
                    .filter(identity_provider_shares::Column::GranteeIdentityId.eq(identity_id))
                    .one(db)
                    .await?
                    .map(|share| ProviderGrant {
                        created_at: share.created_at,
                    });
                grant.is_some()
            }
        };
    if !visible {
        return Ok(None);
    }
    let owner = identities::Entity::find_by_id(&provider.identity_id)
        .one(db)
        .await?
        .ok_or(StorageError::IdentityNotFound)?;
    Ok(Some(VisibleProvider {
        owner_identity_id: owner.id,
        owner_display_name: owner.display_name,
        is_owner,
        visibility,
        grant,
        provider,
    }))
}

async fn provider_sharing_users(
    db: &DatabaseConnection,
    provider_id: &str,
) -> Result<Vec<ProviderSharingUserResponse>, StorageError> {
    let shares = identity_provider_shares::Entity::find()
        .filter(identity_provider_shares::Column::ProviderId.eq(provider_id))
        .order_by_asc(identity_provider_shares::Column::GranteeIdentityId)
        .all(db)
        .await?;
    let identity_ids = shares
        .iter()
        .map(|share| share.grantee_identity_id.clone())
        .collect::<Vec<_>>();
    let display_names = identities::Entity::find()
        .filter(identities::Column::Id.is_in(identity_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|identity| (identity.id, identity.display_name))
        .collect::<std::collections::HashMap<_, _>>();
    shares
        .into_iter()
        .map(|share| {
            let display_name = display_names
                .get(&share.grantee_identity_id)
                .cloned()
                .ok_or(StorageError::IdentityNotFound)?;
            Ok(ProviderSharingUserResponse {
                identity_id: share.grantee_identity_id,
                display_name,
            })
        })
        .collect()
}

async fn validate_sharing_input<C>(
    db: &C,
    owner_identity_id: &str,
    visibility: ProviderVisibility,
    input: &UpdateProviderSharingRequest,
) -> Result<(), StorageError>
where
    C: ConnectionTrait,
{
    if let Some(quota) = &input.quota {
        validate_share_quota(quota)?;
    }
    match visibility {
        ProviderVisibility::Private | ProviderVisibility::All => {
            if !input.users.is_empty() {
                return Err(StorageError::InvalidProviderSharing(
                    "visibility without selected users must not carry users".to_string(),
                ));
            }
            return Ok(());
        }
        ProviderVisibility::Selected if input.users.is_empty() => {
            return Err(StorageError::InvalidProviderSharing(
                "selected visibility requires at least one user".to_string(),
            ));
        }
        ProviderVisibility::Selected => {}
    }

    let mut identities = HashSet::new();
    for user in &input.users {
        if user.identity_id == owner_identity_id {
            return Err(StorageError::InvalidProviderSharing(
                "provider owner cannot be a selected grantee".to_string(),
            ));
        }
        if !identities.insert(user.identity_id.as_str()) {
            return Err(StorageError::InvalidProviderSharing(
                "selected identities must be unique".to_string(),
            ));
        }
    }
    let identity_ids = input
        .users
        .iter()
        .map(|user| user.identity_id.clone())
        .collect::<Vec<_>>();
    let existing = identities::Entity::find()
        .filter(identities::Column::Id.is_in(identity_ids.clone()))
        .all(db)
        .await?;
    if existing.len() != identity_ids.len() {
        return Err(StorageError::InvalidProviderSharing(
            "selected identity does not exist".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prelay_protocol::ShareQuotaAccumulate;

    #[test]
    fn parses_the_rule_object() {
        let rule = parse_share_quota(r#"{"amount":1000,"accumulate":"month"}"#).expect("rule");
        assert_eq!(rule.amount, 1000);
        assert_eq!(rule.accumulate, ShareQuotaAccumulate::Month);
    }

    #[test]
    fn folds_legacy_day_and_week_windows_into_weekly_accumulation() {
        let legacy = r#"[
            {"window":"day","limit":1000,"accumulate":true,"reset_time":"00:00"},
            {"window":"week","limit":7000,"accumulate":false,"reset_time":"00:00","reset_weekday":1}
        ]"#;
        let rule = parse_share_quota(legacy).expect("legacy rule");
        assert_eq!(rule.amount, 1000);
        assert_eq!(rule.accumulate, ShareQuotaAccumulate::Week);
    }

    #[test]
    fn folds_legacy_day_only_rules_by_their_accumulate_flag() {
        let plain = parse_share_quota(r#"[{"window":"day","limit":500,"accumulate":false}]"#)
            .expect("rule");
        assert_eq!(plain.amount, 500);
        assert_eq!(plain.accumulate, ShareQuotaAccumulate::None);

        let carried =
            parse_share_quota(r#"[{"window":"day","limit":500,"accumulate":true}]"#).expect("rule");
        assert_eq!(carried.amount, 500);
        assert_eq!(carried.accumulate, ShareQuotaAccumulate::Week);
    }

    #[test]
    fn keeps_non_accumulating_legacy_shares_that_carry_a_week_total() {
        let legacy = r#"[
            {"window":"day","limit":100000000,"accumulate":false,"reset_time":"00:00"},
            {"window":"week","limit":700000000,"accumulate":false,"reset_time":"00:00","reset_weekday":1}
        ]"#;
        let rule = parse_share_quota(legacy).expect("legacy rule");
        assert_eq!(rule.amount, 100_000_000);
        assert_eq!(rule.accumulate, ShareQuotaAccumulate::None);
    }

    #[test]
    fn rejects_legacy_totals_below_the_daily_derivation() {
        let tight_week = r#"[
            {"window":"day","limit":1000,"accumulate":true},
            {"window":"week","limit":3000,"accumulate":false}
        ]"#;
        assert!(parse_share_quota(tight_week).is_err());

        let tight_month = r#"[
            {"window":"day","limit":1000,"accumulate":true},
            {"window":"month","limit":10000,"accumulate":false}
        ]"#;
        assert!(parse_share_quota(tight_month).is_err());
    }

    #[test]
    fn derives_the_daily_amount_from_a_period_only_rule() {
        let month = parse_share_quota(r#"[{"window":"month","limit":3000,"accumulate":false}]"#)
            .expect("rule");
        assert_eq!(month.amount, 100);
        assert_eq!(month.accumulate, ShareQuotaAccumulate::Month);
    }

    #[test]
    fn rejects_rules_without_a_positive_amount() {
        assert!(parse_share_quota(r#"{"amount":0,"accumulate":"none"}"#).is_err());
        assert!(parse_share_quota(r#"[{"window":"day","limit":0,"accumulate":false}]"#).is_err());
    }
}
