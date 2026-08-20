use chrono::Utc;
use prelay_protocol::{
    CreateProviderRequest, ProviderCapabilityOverrides, ProviderModelResponse, ProviderResponse,
    UpdateProviderRequest,
};
use sqlx::SqlitePool;
use std::collections::HashSet;
use uuid::Uuid;

use super::{crypto::KeyCipher, StorageError};
use crate::providers::spec::resolved_upstream_protocols;

pub(crate) async fn create(
    pool: &SqlitePool,
    crypto: &KeyCipher,
    identity_id: &str,
    input: CreateProviderRequest,
) -> Result<String, StorageError> {
    validate_model_names(&input.models)?;
    let provider_id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let api_key_ciphertext = crypto.encrypt(&input.api_key)?;
    let capabilities_json = input
        .capabilities
        .map(|capabilities| serde_json::to_string(&capabilities))
        .transpose()
        .map_err(|error| StorageError::Crypto(error.to_string()))?;
    let mut transaction = pool.begin().await?;
    let identity_exists =
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM identities WHERE id = ?)")
            .bind(identity_id)
            .fetch_one(&mut *transaction)
            .await?;
    if identity_exists == 0 {
        return Err(StorageError::IdentityNotFound);
    }
    sqlx::query(
        "INSERT INTO identity_provider_configs (\
            id, identity_id, name, provider_type, base_url, api_key_ciphertext, capabilities_json, created_at\
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&provider_id)
    .bind(identity_id)
    .bind(input.name.trim())
    .bind(input.provider_type.trim())
    .bind(input.base_url.trim())
    .bind(api_key_ciphertext)
    .bind(capabilities_json)
    .bind(&created_at)
    .execute(&mut *transaction)
    .await?;
    for model_name in input.models {
        let model_name = model_name.trim();
        if model_name.is_empty() {
            continue;
        }
        sqlx::query(
            "INSERT INTO identity_provider_models (id, provider_id, model_name, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&provider_id)
        .bind(model_name)
        .bind(&created_at)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(provider_id)
}

pub(crate) async fn raw_key_ciphertext(
    pool: &SqlitePool,
    identity_id: &str,
    provider_id: &str,
) -> Result<String, StorageError> {
    sqlx::query_scalar(
        "SELECT api_key_ciphertext FROM identity_provider_configs WHERE id = ? AND identity_id = ?",
    )
    .bind(provider_id)
    .bind(identity_id)
    .fetch_optional(pool)
    .await?
    .ok_or(StorageError::ProviderNotFound)
}

pub(crate) async fn list(
    pool: &SqlitePool,
    identity_id: &str,
) -> Result<Vec<ProviderResponse>, StorageError> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM identity_provider_configs WHERE identity_id = ? ORDER BY created_at",
    )
    .bind(identity_id)
    .fetch_all(pool)
    .await?;
    let mut providers = Vec::with_capacity(ids.len());
    for id in ids {
        providers.push(get(pool, identity_id, &id).await?);
    }
    Ok(providers)
}

pub(crate) async fn get(
    pool: &SqlitePool,
    identity_id: &str,
    provider_id: &str,
) -> Result<ProviderResponse, StorageError> {
    let row = sqlx::query_as::<_, ProviderRow>(
        "SELECT id, name, provider_type, base_url, api_key_ciphertext, capabilities_json, created_at \
         FROM identity_provider_configs WHERE id = ? AND identity_id = ?",
    )
    .bind(provider_id)
    .bind(identity_id)
    .fetch_optional(pool)
    .await?
    .ok_or(StorageError::ProviderNotFound)?;
    let models = sqlx::query_as::<_, ProviderModelRow>(
        "SELECT id, provider_id, model_name, created_at FROM identity_provider_models \
         WHERE provider_id = ? ORDER BY created_at",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    let capabilities: ProviderCapabilityOverrides = row
        .capabilities_json
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default();
    let upstream_protocols = resolved_upstream_protocols(
        &row.provider_type,
        capabilities.upstream_protocols.as_deref(),
    );
    Ok(ProviderResponse {
        id: row.id,
        name: row.name,
        provider_type: row.provider_type,
        base_url: row.base_url,
        api_key_masked: mask_ciphertext(&row.api_key_ciphertext),
        capabilities,
        upstream_protocols,
        models,
        created_at: row.created_at,
    })
}

pub(crate) async fn update(
    pool: &SqlitePool,
    crypto: &KeyCipher,
    identity_id: &str,
    provider_id: &str,
    input: UpdateProviderRequest,
) -> Result<ProviderResponse, StorageError> {
    let existing = sqlx::query_as::<_, ProviderRow>(
        "SELECT id, name, provider_type, base_url, api_key_ciphertext, capabilities_json, created_at \
         FROM identity_provider_configs WHERE id = ? AND identity_id = ?",
    )
    .bind(provider_id)
    .bind(identity_id)
    .fetch_optional(pool)
    .await?
    .ok_or(StorageError::ProviderNotFound)?;
    let capabilities_json = match input.capabilities {
        Some(capabilities) => Some(
            serde_json::to_string(&capabilities)
                .map_err(|error| StorageError::Crypto(error.to_string()))?,
        ),
        None => existing.capabilities_json,
    };
    let api_key_ciphertext = input
        .api_key
        .as_deref()
        .map(|key| crypto.encrypt(key))
        .transpose()?
        .unwrap_or(existing.api_key_ciphertext);
    if let Some(models) = input.models.as_ref() {
        validate_model_names(models)?;
    }
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "UPDATE identity_provider_configs SET name = ?, provider_type = ?, base_url = ?, \
         api_key_ciphertext = ?, capabilities_json = ? WHERE id = ? AND identity_id = ?",
    )
    .bind(
        input
            .name
            .as_deref()
            .map(str::trim)
            .unwrap_or(&existing.name),
    )
    .bind(
        input
            .provider_type
            .as_deref()
            .map(str::trim)
            .unwrap_or(&existing.provider_type),
    )
    .bind(
        input
            .base_url
            .as_deref()
            .map(str::trim)
            .unwrap_or(&existing.base_url),
    )
    .bind(api_key_ciphertext)
    .bind(capabilities_json)
    .bind(provider_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    if let Some(models) = input.models {
        sqlx::query("DELETE FROM identity_provider_models WHERE provider_id = ?")
            .bind(provider_id)
            .execute(&mut *transaction)
            .await?;
        let created_at = Utc::now().to_rfc3339();
        for model_name in models {
            let model_name = model_name.trim();
            if model_name.is_empty() {
                continue;
            }
            sqlx::query(
                "INSERT INTO identity_provider_models (id, provider_id, model_name, created_at) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(provider_id)
            .bind(model_name)
            .bind(&created_at)
            .execute(&mut *transaction)
            .await?;
        }
    }
    transaction.commit().await?;
    get(pool, identity_id, provider_id).await
}

fn validate_model_names(models: &[String]) -> Result<(), StorageError> {
    let mut names = HashSet::with_capacity(models.len());
    for model_name in models {
        let model_name = model_name.trim();
        if !model_name.is_empty() && !names.insert(model_name) {
            return Err(StorageError::ValidationFailed(
                "provider model names must be unique".to_string(),
            ));
        }
    }
    Ok(())
}

pub(crate) async fn delete(
    pool: &SqlitePool,
    identity_id: &str,
    provider_id: &str,
) -> Result<(), StorageError> {
    let mut transaction = pool.begin().await?;
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM identity_provider_configs WHERE id = ? AND identity_id = ?)",
    )
    .bind(provider_id)
    .bind(identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if exists == 0 {
        return Err(StorageError::ProviderNotFound);
    }
    sqlx::query("DELETE FROM identity_interface_models WHERE provider_id = ?")
        .bind(provider_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM identity_provider_models WHERE provider_id = ?")
        .bind(provider_id)
        .execute(&mut *transaction)
        .await?;
    let result =
        sqlx::query("DELETE FROM identity_provider_configs WHERE id = ? AND identity_id = ?")
            .bind(provider_id)
            .bind(identity_id)
            .execute(&mut *transaction)
            .await?;
    if result.rows_affected() == 0 {
        return Err(StorageError::ProviderNotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn add_models(
    pool: &SqlitePool,
    identity_id: &str,
    provider_id: &str,
    model_names: &[String],
) -> Result<(), StorageError> {
    validate_model_names(model_names)?;
    let mut transaction = pool.begin().await?;
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM identity_provider_configs WHERE id = ? AND identity_id = ?)",
    )
    .bind(provider_id)
    .bind(identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if exists == 0 {
        return Err(StorageError::ProviderNotFound);
    }

    let created_at = Utc::now().to_rfc3339();
    for model_name in model_names {
        let model_name = model_name.trim();
        if model_name.is_empty() {
            continue;
        }
        sqlx::query(
            "INSERT OR IGNORE INTO identity_provider_models (id, provider_id, model_name, created_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(provider_id)
        .bind(model_name)
        .bind(&created_at)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ProviderRow {
    id: String,
    name: String,
    provider_type: String,
    base_url: String,
    api_key_ciphertext: String,
    capabilities_json: Option<String>,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct ProviderModelRow {
    id: String,
    provider_id: String,
    model_name: String,
    created_at: String,
}

impl From<ProviderModelRow> for ProviderModelResponse {
    fn from(value: ProviderModelRow) -> Self {
        Self {
            id: value.id,
            provider_id: value.provider_id,
            model_name: value.model_name,
            created_at: value.created_at,
        }
    }
}

fn mask_ciphertext(ciphertext: &str) -> String {
    if ciphertext.is_empty() {
        String::new()
    } else {
        "********".to_string()
    }
}
