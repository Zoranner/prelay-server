use std::collections::HashSet;

use chrono::Utc;
use prelay_protocol::{
    CreateProviderRequest, ProviderCapabilityOverrides, ProviderModelResponse, ProviderResponse,
    UpdateProviderRequest,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, TransactionTrait,
};
use uuid::Uuid;

use crate::{
    entity::{
        identities, identity_endpoint_model_routes, identity_endpoint_models,
        identity_provider_configs, identity_provider_models,
    },
    providers::spec::resolved_upstream_protocols,
};

use super::{crypto::KeyCipher, StorageError};

pub(crate) async fn create(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    input: CreateProviderRequest,
) -> Result<String, StorageError> {
    let models = normalize_model_names(&input.models)?;
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
) -> Result<Vec<ProviderResponse>, StorageError> {
    let providers = identity_provider_configs::Entity::find()
        .filter(identity_provider_configs::Column::IdentityId.eq(identity_id))
        .order_by_asc(identity_provider_configs::Column::CreatedAt)
        .all(db)
        .await?;
    let mut responses = Vec::with_capacity(providers.len());
    for provider in providers {
        responses.push(provider_response(db, crypto, provider).await?);
    }
    Ok(responses)
}

pub(crate) async fn get(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    provider_id: &str,
) -> Result<ProviderResponse, StorageError> {
    let provider = find_provider(db, identity_id, provider_id).await?;
    provider_response(db, crypto, provider).await
}

pub(crate) async fn update(
    db: &DatabaseConnection,
    crypto: &KeyCipher,
    identity_id: &str,
    provider_id: &str,
    input: UpdateProviderRequest,
) -> Result<ProviderResponse, StorageError> {
    let existing = find_provider(db, identity_id, provider_id).await?;
    let models = input
        .models
        .as_deref()
        .map(normalize_model_names)
        .transpose()?;
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
    get(db, crypto, identity_id, provider_id).await
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

async fn provider_response<C>(
    db: &C,
    crypto: &KeyCipher,
    provider: identity_provider_configs::Model,
) -> Result<ProviderResponse, StorageError>
where
    C: ConnectionTrait,
{
    let models = identity_provider_models::Entity::find()
        .filter(identity_provider_models::Column::ProviderId.eq(&provider.id))
        .order_by_asc(identity_provider_models::Column::CreatedAt)
        .all(db)
        .await?
        .into_iter()
        .map(provider_model_response)
        .collect();
    let capabilities: ProviderCapabilityOverrides = provider
        .capabilities_json
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default();
    let upstream_protocols = resolved_upstream_protocols(
        &provider.provider_type,
        capabilities.upstream_protocols.as_deref(),
    );
    let api_key = crypto.decrypt(&provider.api_key_ciphertext)?;
    Ok(ProviderResponse {
        id: provider.id,
        name: provider.name,
        provider_type: provider.provider_type,
        base_url: provider.base_url,
        api_key,
        api_key_masked: mask_ciphertext(&provider.api_key_ciphertext),
        capabilities,
        upstream_protocols,
        models,
        created_at: provider.created_at,
    })
}

fn normalize_model_names(models: &[String]) -> Result<Vec<String>, StorageError> {
    let mut names = HashSet::with_capacity(models.len());
    let mut normalized = Vec::with_capacity(models.len());
    for model_name in models {
        let model_name = model_name.trim();
        if model_name.is_empty() {
            continue;
        }
        if !names.insert(model_name.to_string()) {
            return Err(StorageError::ValidationFailed(
                "provider model names must be unique".to_string(),
            ));
        }
        normalized.push(model_name.to_string());
    }
    Ok(normalized)
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

fn provider_model_response(model: identity_provider_models::Model) -> ProviderModelResponse {
    ProviderModelResponse {
        id: model.id,
        provider_id: model.provider_id,
        model_name: model.model_name,
        created_at: model.created_at,
    }
}

fn mask_ciphertext(ciphertext: &str) -> String {
    if ciphertext.is_empty() {
        String::new()
    } else {
        "********".to_string()
    }
}
