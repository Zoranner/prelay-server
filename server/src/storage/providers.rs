use chrono::Utc;
use provider_relay_protocol::CreateProviderRequest;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{crypto::KeyCipher, StorageError};

pub(crate) async fn create(
    pool: &SqlitePool,
    crypto: &KeyCipher,
    identity_id: &str,
    input: CreateProviderRequest,
) -> Result<String, StorageError> {
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
