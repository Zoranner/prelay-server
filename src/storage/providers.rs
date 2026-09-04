use std::collections::HashSet;

use chrono::Utc;
use prelay_protocol::{CreateProviderRequest, ProviderResponse, UpdateProviderRequest};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, TransactionTrait,
};
use uuid::Uuid;

use crate::{
    entity::{
        identities,
        identity::{
            endpoint_model_routes as identity_endpoint_model_routes,
            endpoint_models as identity_endpoint_models,
            provider_configs as identity_provider_configs,
            provider_models as identity_provider_models,
        },
    },
    provider_catalog::ProviderCatalog,
};

use super::{
    crypto::KeyCipher,
    provider_validation::{normalize_model_names, validate_catalog_provider},
    provider_views::provider_response,
    Storage, StorageError,
};

impl Storage {
    pub async fn create_provider(
        &self,
        identity_id: &str,
        input: CreateProviderRequest,
    ) -> Result<String, StorageError> {
        create(&self.db, &self.crypto, identity_id, input).await
    }

    pub async fn create_provider_with_catalog(
        &self,
        identity_id: &str,
        input: CreateProviderRequest,
        catalog: &ProviderCatalog,
    ) -> Result<String, StorageError> {
        create_with_catalog(&self.db, &self.crypto, identity_id, input, catalog).await
    }

    pub async fn raw_provider_key_ciphertext(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<String, StorageError> {
        raw_key_ciphertext(&self.db, identity_id, provider_id).await
    }

    pub async fn decrypt_provider_key(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<String, StorageError> {
        let ciphertext = self
            .raw_provider_key_ciphertext(identity_id, provider_id)
            .await?;
        self.crypto.decrypt(&ciphertext)
    }

    pub async fn list_providers(
        &self,
        identity_id: &str,
    ) -> Result<Vec<ProviderResponse>, StorageError> {
        list(&self.db, &self.crypto, identity_id, None).await
    }

    pub async fn list_providers_with_catalog(
        &self,
        identity_id: &str,
        catalog: &ProviderCatalog,
    ) -> Result<Vec<ProviderResponse>, StorageError> {
        list(&self.db, &self.crypto, identity_id, Some(catalog)).await
    }

    pub async fn get_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<ProviderResponse, StorageError> {
        get(&self.db, &self.crypto, identity_id, provider_id, None).await
    }

    pub async fn get_provider_with_catalog(
        &self,
        identity_id: &str,
        provider_id: &str,
        catalog: &ProviderCatalog,
    ) -> Result<ProviderResponse, StorageError> {
        get(
            &self.db,
            &self.crypto,
            identity_id,
            provider_id,
            Some(catalog),
        )
        .await
    }

    pub async fn update_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
        input: UpdateProviderRequest,
    ) -> Result<ProviderResponse, StorageError> {
        update(
            &self.db,
            &self.crypto,
            identity_id,
            provider_id,
            input,
            None,
        )
        .await
    }

    pub async fn update_provider_with_catalog(
        &self,
        identity_id: &str,
        provider_id: &str,
        input: UpdateProviderRequest,
        catalog: &ProviderCatalog,
    ) -> Result<ProviderResponse, StorageError> {
        update(
            &self.db,
            &self.crypto,
            identity_id,
            provider_id,
            input,
            Some(catalog),
        )
        .await
    }

    pub async fn delete_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<(), StorageError> {
        delete(&self.db, identity_id, provider_id).await
    }

    pub async fn add_provider_models(
        &self,
        identity_id: &str,
        provider_id: &str,
        model_names: &[String],
    ) -> Result<(), StorageError> {
        add_models(&self.db, identity_id, provider_id, model_names).await
    }
}

pub(crate) async fn create(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    input: CreateProviderRequest,
) -> Result<String, StorageError> {
    create_inner(db, crypto, identity_id, input, None).await
}

async fn create_with_catalog(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    input: CreateProviderRequest,
    catalog: &ProviderCatalog,
) -> Result<String, StorageError> {
    create_inner(db, crypto, identity_id, input, Some(catalog)).await
}

async fn create_inner(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    input: CreateProviderRequest,
    catalog: Option<&ProviderCatalog>,
) -> Result<String, StorageError> {
    let models = normalize_model_names(&input.models)?;
    if let Some(catalog) = catalog {
        validate_catalog_provider(catalog, &input.provider_type, &models)?;
    }
    let provider_id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let api_key_ciphertext = crypto.encrypt(&input.api_key)?;
    let capabilities_json = input
        .capabilities
        .map(|capabilities| serde_json::to_string(&capabilities))
        .transpose()
        .map_err(|error| StorageError::Crypto(error.to_string()))?;
    let transaction = db.begin().await?;
    if identities::Entity::find_by_id(identity_id)
        .one(&transaction)
        .await?
        .is_none()
    {
        return Err(StorageError::IdentityNotFound);
    }

    identity_provider_configs::ActiveModel {
        id: Set(provider_id.clone()),
        identity_id: Set(identity_id.to_string()),
        name: Set(input.name.trim().to_string()),
        provider_type: Set(input.provider_type.trim().to_string()),
        base_url: Set(input.base_url.trim().to_string()),
        api_key_ciphertext: Set(api_key_ciphertext),
        capabilities_json: Set(capabilities_json),
        created_at: Set(created_at.clone()),
    }
    .insert(&transaction)
    .await?;
    insert_models(&transaction, &provider_id, models, &created_at).await?;
    transaction.commit().await?;
    Ok(provider_id)
}

pub(crate) async fn raw_key_ciphertext(
    db: &DatabaseConnection,
    identity_id: &str,
    provider_id: &str,
) -> Result<String, StorageError> {
    Ok(find_provider(db, identity_id, provider_id)
        .await?
        .api_key_ciphertext)
}

pub(crate) async fn list(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    catalog: Option<&ProviderCatalog>,
) -> Result<Vec<ProviderResponse>, StorageError> {
    let providers = identity_provider_configs::Entity::find()
        .filter(identity_provider_configs::Column::IdentityId.eq(identity_id))
        .order_by_asc(identity_provider_configs::Column::CreatedAt)
        .all(db)
        .await?;
    let mut responses = Vec::with_capacity(providers.len());
    for provider in providers {
        responses.push(provider_response(db, crypto, provider, catalog).await?);
    }
    Ok(responses)
}

pub(crate) async fn get(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    provider_id: &str,
    catalog: Option<&ProviderCatalog>,
) -> Result<ProviderResponse, StorageError> {
    let provider = find_provider(db, identity_id, provider_id).await?;
    provider_response(db, crypto, provider, catalog).await
}

pub(crate) async fn update(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    provider_id: &str,
    input: UpdateProviderRequest,
    catalog: Option<&ProviderCatalog>,
) -> Result<ProviderResponse, StorageError> {
    let existing = find_provider(db, identity_id, provider_id).await?;
    let models = input
        .models
        .as_deref()
        .map(normalize_model_names)
        .transpose()?;
    if let Some(catalog) = catalog {
        let provider_type = input
            .provider_type
            .as_deref()
            .unwrap_or(&existing.provider_type);
        let models_to_validate = match models.as_deref() {
            Some(models) => models.to_vec(),
            None => identity_provider_models::Entity::find()
                .filter(identity_provider_models::Column::ProviderId.eq(provider_id))
                .all(db)
                .await?
                .into_iter()
                .map(|model| model.model_name)
                .collect(),
        };
        validate_catalog_provider(catalog, provider_type, &models_to_validate)?;
    }
    let capabilities_json = match input.capabilities {
        Some(capabilities) => Some(
            serde_json::to_string(&capabilities)
                .map_err(|error| StorageError::Crypto(error.to_string()))?,
        ),
        None => existing.capabilities_json.clone(),
    };
    let api_key_ciphertext = input
        .api_key
        .as_deref()
        .map(|key| crypto.encrypt(key))
        .transpose()?
        .unwrap_or_else(|| existing.api_key_ciphertext.clone());

    let transaction = db.begin().await?;
    let mut active = existing.into_active_model();
    if let Some(name) = input.name {
        active.name = Set(name.trim().to_string());
    }
    if let Some(provider_type) = input.provider_type {
        active.provider_type = Set(provider_type.trim().to_string());
    }
    if let Some(base_url) = input.base_url {
        active.base_url = Set(base_url.trim().to_string());
    }
    active.api_key_ciphertext = Set(api_key_ciphertext);
    active.capabilities_json = Set(capabilities_json);
    active.update(&transaction).await?;

    if let Some(models) = models {
        identity_provider_models::Entity::delete_many()
            .filter(identity_provider_models::Column::ProviderId.eq(provider_id))
            .exec(&transaction)
            .await?;
        insert_models(&transaction, provider_id, models, &Utc::now().to_rfc3339()).await?;
    }
    transaction.commit().await?;
    get(db, crypto, identity_id, provider_id, catalog).await
}

pub(crate) async fn delete(
    db: &DatabaseConnection,
    identity_id: &str,
    provider_id: &str,
) -> Result<(), StorageError> {
    let transaction = db.begin().await?;
    find_provider(&transaction, identity_id, provider_id).await?;
    identity_endpoint_model_routes::Entity::delete_many()
        .filter(identity_endpoint_model_routes::Column::ProviderId.eq(provider_id))
        .exec(&transaction)
        .await?;
    identity_endpoint_models::Entity::delete_many()
        .filter(identity_endpoint_models::Column::ProviderId.eq(provider_id))
        .exec(&transaction)
        .await?;
    identity_provider_models::Entity::delete_many()
        .filter(identity_provider_models::Column::ProviderId.eq(provider_id))
        .exec(&transaction)
        .await?;
    identity_provider_configs::Entity::delete_by_id(provider_id.to_string())
        .exec(&transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn add_models(
    db: &DatabaseConnection,
    identity_id: &str,
    provider_id: &str,
    model_names: &[String],
) -> Result<(), StorageError> {
    let model_names = normalize_model_names(model_names)?;
    let transaction = db.begin().await?;
    find_provider(&transaction, identity_id, provider_id).await?;
    let existing = identity_provider_models::Entity::find()
        .filter(identity_provider_models::Column::ProviderId.eq(provider_id))
        .all(&transaction)
        .await?
        .into_iter()
        .map(|model| model.model_name)
        .collect::<HashSet<_>>();
    let model_names = model_names
        .into_iter()
        .filter(|model_name| !existing.contains(model_name))
        .collect();
    insert_models(
        &transaction,
        provider_id,
        model_names,
        &Utc::now().to_rfc3339(),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn find_provider<C>(
    db: &C,
    identity_id: &str,
    provider_id: &str,
) -> Result<identity_provider_configs::Model, StorageError>
where
    C: ConnectionTrait,
{
    identity_provider_configs::Entity::find_by_id(provider_id)
        .filter(identity_provider_configs::Column::IdentityId.eq(identity_id))
        .one(db)
        .await?
        .ok_or(StorageError::ProviderNotFound)
}

async fn insert_models<C>(
    db: &C,
    provider_id: &str,
    model_names: Vec<String>,
    created_at: &str,
) -> Result<(), StorageError>
where
    C: ConnectionTrait,
{
    for model_name in model_names {
        identity_provider_models::ActiveModel {
            id: Set(Uuid::new_v4().to_string()),
            provider_id: Set(provider_id.to_string()),
            model_name: Set(model_name),
            created_at: Set(created_at.to_string()),
        }
        .insert(db)
        .await?;
    }
    Ok(())
}
