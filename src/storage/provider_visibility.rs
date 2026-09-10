use std::collections::HashSet;

use chrono::Utc;
use prelay_protocol::{
    ProviderListItemResponse, ProviderSharingResponse, ProviderVisibility,
    UpdateProviderSharingRequest,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, TransactionTrait,
};

use crate::entity::{
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
    pub async fn list_visible_providers(
        &self,
        identity_id: &str,
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
        let selected_identity_ids = selected_identity_ids(&self.db, provider_id).await?;
        Ok(ProviderSharingResponse {
            visibility: provider.visibility,
            selected_identity_ids,
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
        provider.visibility = Set(visibility_value(input.visibility).to_string());
        provider.update(&transaction).await?;

        identity_provider_shares::Entity::delete_many()
            .filter(identity_provider_shares::Column::ProviderId.eq(provider_id))
            .exec(&transaction)
            .await?;
        if input.visibility == ProviderVisibility::Selected {
            let created_at = Utc::now().to_rfc3339();
            for grantee_identity_id in input.identity_ids {
                identity_provider_shares::ActiveModel {
                    provider_id: Set(provider_id.to_string()),
                    grantee_identity_id: Set(grantee_identity_id),
                    created_at: Set(created_at.clone()),
                }
                .insert(&transaction)
                .await?;
            }
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

async fn visible_provider<C>(
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

async fn validate_sharing_input<C>(
    db: &C,
    owner_identity_id: &str,
    input: &UpdateProviderSharingRequest,
) -> Result<(), StorageError>
where
    C: ConnectionTrait,
{
    let identities = input.identity_ids.iter().collect::<HashSet<_>>();
    if identities.len() != input.identity_ids.len() {
        return Err(StorageError::InvalidProviderSharing(
            "selected identities must be unique".to_string(),
        ));
    }
    if input.visibility != ProviderVisibility::Selected && !input.identity_ids.is_empty() {
        return Err(StorageError::InvalidProviderSharing(
            "private and all visibility must not include selected identities".to_string(),
        ));
    }
    if input.visibility != ProviderVisibility::Selected {
        return Ok(());
    }
    if input.identity_ids.iter().any(|id| id == owner_identity_id) {
        return Err(StorageError::InvalidProviderSharing(
            "provider owner cannot be a selected grantee".to_string(),
        ));
    }
    let existing = identities::Entity::find()
        .filter(identities::Column::Id.is_in(input.identity_ids.clone()))
        .all(db)
        .await?;
    if existing.len() != input.identity_ids.len() {
        return Err(StorageError::InvalidProviderSharing(
            "selected identity does not exist".to_string(),
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
