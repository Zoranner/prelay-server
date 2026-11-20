use std::collections::{HashMap, HashSet};

use chrono::Utc;
use prelay_protocol::{
    ProviderResponse, ProviderSharingResponse, ProviderSharingUserResponse, ProviderVisibility,
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

use super::{provider_views::provider_response, Storage, StorageError};

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
                .map(serde_json::from_str::<Vec<ShareQuotaWindow>>)
                .transpose()
                .map_err(|error| {
                    StorageError::InvalidProviderSharing(format!(
                        "stored sharing quota is invalid: {error}"
                    ))
                })?,
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
        let visibility = input.visibility;
        validate_sharing_input(&transaction, identity_id, visibility, &input).await?;

        let mut provider = provider.into_active_model();
        provider.visibility = Set(visibility_value(visibility).to_string());
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
