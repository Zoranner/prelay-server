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
            provider_shares as identity_provider_shares,
        },
    },
    provider_catalog::ProviderCatalog,
};

use super::{
    crypto::KeyCipher, provider_validation::validate_catalog_provider,
    provider_views::provider_response, provider_visibility::owned_provider, Storage, StorageError,
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
        list(&self.db, &self.crypto, identity_id).await
    }

    pub async fn list_providers_with_catalog(
        &self,
        identity_id: &str,
        _catalog: &ProviderCatalog,
    ) -> Result<Vec<ProviderResponse>, StorageError> {
        list(&self.db, &self.crypto, identity_id).await
    }

    pub async fn get_provider(
        &self,
        identity_id: &str,
        provider_id: &str,
    ) -> Result<ProviderResponse, StorageError> {
        get(&self.db, &self.crypto, identity_id, provider_id).await
    }

    pub async fn get_provider_with_catalog(
        &self,
        identity_id: &str,
        provider_id: &str,
        _catalog: &ProviderCatalog,
    ) -> Result<ProviderResponse, StorageError> {
        get(&self.db, &self.crypto, identity_id, provider_id).await
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
    if let Some(catalog) = catalog {
        validate_catalog_provider(catalog, &input.provider_type)?;
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
        visibility: Set("private".to_string()),
        base_url: Set(input.base_url.trim().to_string()),
        api_key_ciphertext: Set(api_key_ciphertext),
        capabilities_json: Set(capabilities_json),
        created_at: Set(created_at.clone()),
    }
    .insert(&transaction)
    .await?;
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
        responses.push(provider_response(crypto, provider)?);
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
    provider_response(crypto, provider)
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
    if let Some(catalog) = catalog {
        let provider_type = input
            .provider_type
            .as_deref()
            .unwrap_or(&existing.provider_type);
        validate_catalog_provider(catalog, provider_type)?;
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

    transaction.commit().await?;
    get(db, crypto, identity_id, provider_id).await
}

pub(crate) async fn delete(
    db: &DatabaseConnection,
    identity_id: &str,
    provider_id: &str,
) -> Result<(), StorageError> {
    let transaction = db.begin().await?;
    owned_provider(&transaction, identity_id, provider_id).await?;
    identity_provider_shares::Entity::delete_many()
        .filter(identity_provider_shares::Column::ProviderId.eq(provider_id))
        .exec(&transaction)
        .await?;
    identity_endpoint_model_routes::Entity::delete_many()
        .filter(identity_endpoint_model_routes::Column::ProviderId.eq(provider_id))
        .exec(&transaction)
        .await?;
    identity_endpoint_models::Entity::delete_many()
        .filter(identity_endpoint_models::Column::ProviderId.eq(provider_id))
        .exec(&transaction)
        .await?;
    identity_provider_configs::Entity::delete_by_id(provider_id.to_string())
        .exec(&transaction)
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
    owned_provider(db, identity_id, provider_id).await
}

#[cfg(test)]
mod tests {
    use sea_orm::EntityTrait;

    use super::*;
    use crate::{
        database::{connect, DatabaseConfig},
        entity::identity::provider_configs as identity_provider_configs,
        identity::credential::generate_credential,
        schema::initialize,
        storage::MasterKey,
    };

    #[tokio::test]
    async fn create_provider_defaults_visibility_to_private() {
        let database_config =
            DatabaseConfig::from_url("sqlite::memory:").expect("valid in-memory SQLite URL");
        let db = connect(&database_config)
            .await
            .expect("connect to in-memory SQLite");
        initialize(&db)
            .await
            .expect("initialize test database schema");
        let storage = Storage::from_connection(db, MasterKey::from_bytes([0; 32]));
        let identity = storage
            .register_identity(
                "machine-provider-visibility",
                "sid-provider-visibility",
                &generate_credential(),
            )
            .await
            .expect("register identity");

        let provider_id = storage
            .create_provider(
                &identity.identity_id,
                CreateProviderRequest {
                    name: "Private provider".to_string(),
                    provider_type: "openai_compatible".to_string(),
                    base_url: "https://provider.example".to_string(),
                    api_key: "provider-key".to_string(),
                    capabilities: None,
                },
            )
            .await
            .expect("create provider");

        let provider = identity_provider_configs::Entity::find_by_id(provider_id)
            .one(&storage.db)
            .await
            .expect("load provider")
            .expect("provider exists");
        assert_eq!(provider.visibility, "private");
    }
}
