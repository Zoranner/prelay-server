use std::collections::{HashMap, HashSet};

use chrono::Utc;
use prelay_protocol::{
    ProviderListItemResponse, ProviderShareResponse, ProviderSharingResponse, ProviderVisibility,
    ShareQuotaWindow, ShareQuotaWindowKind, UpdateProviderSharingRequest,
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

use super::{provider_views::provider_list_item, Storage, StorageError};

#[derive(Clone, Debug)]
pub(crate) struct VisibleProvider {
    pub(crate) provider: identity_provider_configs::Model,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_display_name: String,
    pub(crate) visibility: ProviderVisibility,
    pub(crate) can_manage: bool,
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
    ) -> Result<Vec<ProviderListItemResponse>, StorageError> {
        let providers = identity_provider_configs::Entity::find()
            .order_by_asc(identity_provider_configs::Column::CreatedAt)
            .all(&self.db)
            .await?;
        let mut visible = Vec::new();
        for provider in providers {
            let Some(provider) = visible_provider(&self.db, identity_id, provider).await? else {
                continue;
            };
            let selected_identity_ids = if provider.visibility == ProviderVisibility::Selected {
                selected_identity_ids(&self.db, &provider.provider.id).await?
            } else {
                Vec::new()
            };
            visible.push(provider_list_item(
                catalog,
                provider.provider,
                provider.owner_identity_id,
                provider.owner_display_name,
                provider.visibility,
                selected_identity_ids,
                provider.can_manage,
            )?);
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
        Ok(ProviderSharingResponse {
            shares: provider_shares(&self.db, provider_id).await?,
            can_manage: provider.can_manage,
        })
    }

    pub async fn update_provider_sharing(
        &self,
        identity_id: &str,
        provider_id: &str,
        input: UpdateProviderSharingRequest,
    ) -> Result<ProviderSharingResponse, StorageError> {
        let transaction = self.db.begin().await?;
        let provider = owned_provider(&transaction, identity_id, provider_id).await?;
        validate_sharing_input(&transaction, identity_id, &input).await?;

        let mut provider = provider.into_active_model();
        let visibility = if input.shares.is_empty() {
            ProviderVisibility::Private
        } else {
            ProviderVisibility::Selected
        };
        provider.visibility = Set(visibility_value(visibility).to_string());
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
        for share in input.shares {
            let quota_json = share
                .quota
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| {
                    StorageError::InvalidProviderSharing(format!(
                        "could not serialize share quota: {error}"
                    ))
                })?;
            let created_at = existing_created_at
                .get(&share.identity_id)
                .cloned()
                .unwrap_or_else(|| now.clone());
            identity_provider_shares::ActiveModel {
                provider_id: Set(provider_id.to_string()),
                grantee_identity_id: Set(share.identity_id),
                created_at: Set(created_at),
                quota_json: Set(quota_json),
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
    let visible = is_owner
        || match visibility {
            ProviderVisibility::Private => false,
            ProviderVisibility::All => identities::Entity::find_by_id(identity_id)
                .one(db)
                .await?
                .is_some(),
            ProviderVisibility::Selected => identity_provider_shares::Entity::find()
                .filter(identity_provider_shares::Column::ProviderId.eq(&provider.id))
                .filter(identity_provider_shares::Column::GranteeIdentityId.eq(identity_id))
                .one(db)
                .await?
                .is_some(),
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
        can_manage: is_owner,
        visibility,
        provider,
    }))
}

async fn selected_identity_ids(
    db: &DatabaseConnection,
    provider_id: &str,
) -> Result<Vec<String>, StorageError> {
    Ok(identity_provider_shares::Entity::find()
        .filter(identity_provider_shares::Column::ProviderId.eq(provider_id))
        .order_by_asc(identity_provider_shares::Column::GranteeIdentityId)
        .all(db)
        .await?
        .into_iter()
        .map(|share| share.grantee_identity_id)
        .collect())
}

async fn provider_shares(
    db: &DatabaseConnection,
    provider_id: &str,
) -> Result<Vec<ProviderShareResponse>, StorageError> {
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
            let quota = share
                .quota_json
                .as_deref()
                .map(serde_json::from_str::<Vec<ShareQuotaWindow>>)
                .transpose()
                .map_err(|error| {
                    StorageError::InvalidProviderSharing(format!(
                        "stored share quota is invalid: {error}"
                    ))
                })?;
            Ok(ProviderShareResponse {
                identity_id: share.grantee_identity_id,
                display_name,
                quota,
            })
        })
        .collect()
}

async fn validate_sharing_input<C>(
    db: &C,
    owner_identity_id: &str,
    input: &UpdateProviderSharingRequest,
) -> Result<(), StorageError>
where
    C: ConnectionTrait,
{
    let mut identities = HashSet::new();
    for share in &input.shares {
        if share.identity_id == owner_identity_id {
            return Err(StorageError::InvalidProviderSharing(
                "provider owner cannot be a selected grantee".to_string(),
            ));
        }
        if !identities.insert(share.identity_id.as_str()) {
            return Err(StorageError::InvalidProviderSharing(
                "selected identities must be unique".to_string(),
            ));
        }
        if let Some(quota) = &share.quota {
            validate_share_quota(quota)?;
        }
    }
    let identity_ids = input
        .shares
        .iter()
        .map(|share| share.identity_id.clone())
        .collect::<Vec<_>>();
    if identity_ids.is_empty() {
        return Ok(());
    }
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

fn validate_share_quota(quota: &[ShareQuotaWindow]) -> Result<(), StorageError> {
    if quota.is_empty() {
        return Err(StorageError::InvalidProviderSharing(
            "limited sharing requires at least one quota window".to_string(),
        ));
    }
    let mut windows = HashSet::new();
    for rule in quota {
        if !windows.insert(rule.window) {
            return Err(StorageError::InvalidProviderSharing(
                "quota windows must be unique".to_string(),
            ));
        }
        if rule.limit <= 0 {
            return Err(StorageError::InvalidProviderSharing(
                "quota limit must be positive".to_string(),
            ));
        }
        parse_reset_time(&rule.reset_time)?;
        match rule.window {
            ShareQuotaWindowKind::Day => {
                if rule.reset_weekday.is_some() || rule.reset_day.is_some() {
                    return Err(StorageError::InvalidProviderSharing(
                        "day quota must not set weekday or month day".to_string(),
                    ));
                }
            }
            ShareQuotaWindowKind::Week => {
                if !matches!(rule.reset_weekday, Some(1..=7)) || rule.reset_day.is_some() {
                    return Err(StorageError::InvalidProviderSharing(
                        "week quota requires a weekday from 1 to 7".to_string(),
                    ));
                }
            }
            ShareQuotaWindowKind::Month => {
                if !matches!(rule.reset_day, Some(1..=28)) || rule.reset_weekday.is_some() {
                    return Err(StorageError::InvalidProviderSharing(
                        "month quota requires a day from 1 to 28".to_string(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn parse_reset_time(value: &str) -> Result<(), StorageError> {
    let (hour, minute) = value.split_once(':').ok_or_else(|| {
        StorageError::InvalidProviderSharing("reset time must use HH:MM".to_string())
    })?;
    let hour = hour.parse::<u8>().map_err(|_| {
        StorageError::InvalidProviderSharing("reset time must use HH:MM".to_string())
    })?;
    let minute = minute.parse::<u8>().map_err(|_| {
        StorageError::InvalidProviderSharing("reset time must use HH:MM".to_string())
    })?;
    if hour > 23 || minute > 59 {
        return Err(StorageError::InvalidProviderSharing(
            "reset time must use HH:MM".to_string(),
        ));
    }
    Ok(())
}

fn parse_visibility(value: &str) -> Result<ProviderVisibility, StorageError> {
    match value {
        "private" => Ok(ProviderVisibility::Private),
        "selected" => Ok(ProviderVisibility::Selected),
        "all" => Ok(ProviderVisibility::All),
        other => Err(StorageError::InvalidProviderSharing(format!(
            "unknown visibility {other}"
        ))),
    }
}

const fn visibility_value(visibility: ProviderVisibility) -> &'static str {
    match visibility {
        ProviderVisibility::Private => "private",
        ProviderVisibility::Selected => "selected",
        ProviderVisibility::All => "all",
    }
}
