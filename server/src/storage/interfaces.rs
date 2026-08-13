use chrono::Utc;
use provider_relay_protocol::{
    CreateInterfaceRequest, InterfaceModelInput, InterfaceModelResponse, InterfaceResponse,
    UpdateInterfaceRequest,
};
use sqlx::SqlitePool;
use std::collections::HashSet;
use uuid::Uuid;

use crate::identity::credential::generate_credential;

use super::StorageError;

pub(crate) async fn create(
    pool: &SqlitePool,
    identity_id: &str,
    input: CreateInterfaceRequest,
) -> Result<InterfaceResponse, StorageError> {
    validate_model_names(&input.models)?;
    let interface_id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let protocol = input.protocol.unwrap_or_else(|| "all".to_string());
    let mut transaction = pool.begin().await?;
    ensure_identity_exists(&mut transaction, identity_id).await?;
    sqlx::query(
        "INSERT INTO identity_interface_configs (id, identity_id, name, protocol, token, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&interface_id)
    .bind(identity_id)
    .bind(input.name.trim())
    .bind(protocol.trim())
    .bind(generate_credential())
    .bind(&created_at)
    .execute(&mut *transaction)
    .await?;
    replace_models(
        &mut transaction,
        identity_id,
        &interface_id,
        input.models,
        &created_at,
    )
    .await?;
    transaction.commit().await?;
    get(pool, identity_id, &interface_id).await
}

pub(crate) async fn list(
    pool: &SqlitePool,
    identity_id: &str,
) -> Result<Vec<InterfaceResponse>, StorageError> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM identity_interface_configs WHERE identity_id = ? ORDER BY created_at",
    )
    .bind(identity_id)
    .fetch_all(pool)
    .await?;
    let mut interfaces = Vec::with_capacity(ids.len());
    for id in ids {
        interfaces.push(get(pool, identity_id, &id).await?);
    }
    Ok(interfaces)
}

pub(crate) async fn get(
    pool: &SqlitePool,
    identity_id: &str,
    interface_id: &str,
) -> Result<InterfaceResponse, StorageError> {
    let interface = sqlx::query_as::<_, InterfaceRow>(
        "SELECT id, name, protocol, token, created_at FROM identity_interface_configs \
         WHERE id = ? AND identity_id = ?",
    )
    .bind(interface_id)
    .bind(identity_id)
    .fetch_optional(pool)
    .await?
    .ok_or(StorageError::InterfaceNotFound)?;
    let models = sqlx::query_as::<_, InterfaceModelRow>(
        "SELECT id, interface_id, model_name, provider_id, upstream_model, created_at \
         FROM identity_interface_models WHERE interface_id = ? ORDER BY created_at",
    )
    .bind(interface_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    Ok(InterfaceResponse {
        id: interface.id,
        name: interface.name,
        protocol: interface.protocol,
        token: interface.token,
        models,
        created_at: interface.created_at,
    })
}

pub(crate) async fn update(
    pool: &SqlitePool,
    identity_id: &str,
    interface_id: &str,
    input: UpdateInterfaceRequest,
) -> Result<InterfaceResponse, StorageError> {
    let current = get(pool, identity_id, interface_id).await?;
    if let Some(models) = input.models.as_ref() {
        validate_model_names(models)?;
    }
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "UPDATE identity_interface_configs SET name = ?, protocol = ? WHERE id = ? AND identity_id = ?",
    )
    .bind(input.name.as_deref().map(str::trim).unwrap_or(&current.name))
    .bind(
        input
            .protocol
            .as_deref()
            .map(str::trim)
            .unwrap_or(&current.protocol),
    )
    .bind(interface_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    if let Some(models) = input.models {
        replace_models(
            &mut transaction,
            identity_id,
            interface_id,
            models,
            &Utc::now().to_rfc3339(),
        )
        .await?;
    }
    transaction.commit().await?;
    get(pool, identity_id, interface_id).await
}

fn validate_model_names(models: &[InterfaceModelInput]) -> Result<(), StorageError> {
    let mut names = HashSet::with_capacity(models.len());
    for model in models {
        let model_name = model
            .model_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| model.upstream_model.trim());
        if !names.insert(model_name) {
            return Err(StorageError::ValidationFailed(
                "interface model names must be unique".to_string(),
            ));
        }
    }
    Ok(())
}

pub(crate) async fn delete(
    pool: &SqlitePool,
    identity_id: &str,
    interface_id: &str,
) -> Result<(), StorageError> {
    let mut transaction = pool.begin().await?;
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM identity_interface_configs WHERE id = ? AND identity_id = ?)",
    )
    .bind(interface_id)
    .bind(identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if exists == 0 {
        return Err(StorageError::InterfaceNotFound);
    }
    sqlx::query(
        "DELETE FROM identity_interface_models WHERE interface_id = ? \
         AND EXISTS (SELECT 1 FROM identity_interface_configs WHERE id = ? AND identity_id = ?)",
    )
    .bind(interface_id)
    .bind(interface_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM identity_interface_configs WHERE id = ? AND identity_id = ?")
        .bind(interface_id)
        .bind(identity_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn regenerate_token(
    pool: &SqlitePool,
    identity_id: &str,
    interface_id: &str,
) -> Result<InterfaceResponse, StorageError> {
    let result = sqlx::query(
        "UPDATE identity_interface_configs SET token = ? WHERE id = ? AND identity_id = ?",
    )
    .bind(generate_credential())
    .bind(interface_id)
    .bind(identity_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(StorageError::InterfaceNotFound);
    }
    get(pool, identity_id, interface_id).await
}

async fn ensure_identity_exists(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    identity_id: &str,
) -> Result<(), StorageError> {
    let exists =
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM identities WHERE id = ?)")
            .bind(identity_id)
            .fetch_one(&mut **transaction)
            .await?;
    if exists == 0 {
        return Err(StorageError::IdentityNotFound);
    }
    Ok(())
}

async fn replace_models(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    identity_id: &str,
    interface_id: &str,
    models: Vec<InterfaceModelInput>,
    created_at: &str,
) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM identity_interface_models WHERE interface_id = ?")
        .bind(interface_id)
        .execute(&mut **transaction)
        .await?;
    for model in models {
        let provider_exists = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM identity_provider_configs WHERE id = ? AND identity_id = ?)",
        )
        .bind(&model.provider_id)
        .bind(identity_id)
        .fetch_one(&mut **transaction)
        .await?;
        if provider_exists == 0 {
            return Err(StorageError::ProviderNotFound);
        }
        let upstream_model = model.upstream_model.trim();
        let model_exists = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM identity_provider_models WHERE provider_id = ? AND model_name = ?)",
        )
        .bind(&model.provider_id)
        .bind(upstream_model)
        .fetch_one(&mut **transaction)
        .await?;
        if model_exists == 0 {
            return Err(StorageError::ProviderNotFound);
        }
        let model_name = model
            .model_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(upstream_model);
        sqlx::query(
            "INSERT INTO identity_interface_models \
             (id, interface_id, model_name, provider_id, upstream_model, created_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(interface_id)
        .bind(model_name)
        .bind(&model.provider_id)
        .bind(upstream_model)
        .bind(created_at)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct InterfaceRow {
    id: String,
    name: String,
    protocol: String,
    token: String,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct InterfaceModelRow {
    id: String,
    interface_id: String,
    model_name: String,
    provider_id: String,
    upstream_model: String,
    created_at: String,
}

impl From<InterfaceModelRow> for InterfaceModelResponse {
    fn from(value: InterfaceModelRow) -> Self {
        Self {
            id: value.id,
            interface_id: value.interface_id,
            model_name: value.model_name,
            provider_id: value.provider_id,
            upstream_model: value.upstream_model,
            created_at: value.created_at,
        }
    }
}
