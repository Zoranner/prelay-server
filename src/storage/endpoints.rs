use chrono::Utc;
use prelay_protocol::{
    CreateEndpointRequest, EndpointModelInput, EndpointModelResponse, EndpointResponse,
    UpdateEndpointRequest,
};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::identity::credential::generate_credential;

use super::StorageError;

pub(crate) async fn create(
    pool: &SqlitePool,
    identity_id: &str,
    input: CreateEndpointRequest,
) -> Result<EndpointResponse, StorageError> {
    let endpoint_id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let protocol = input.protocol.unwrap_or_else(|| "all".to_string());
    let mut transaction = pool.begin().await?;
    ensure_identity_exists(&mut transaction, identity_id).await?;
    sqlx::query(
        "INSERT INTO identity_endpoint_configs (id, identity_id, name, protocol, token, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&endpoint_id)
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
        &endpoint_id,
        input.models,
        &created_at,
    )
    .await?;
    transaction.commit().await?;
    get(pool, identity_id, &endpoint_id).await
}

pub(crate) async fn list(
    pool: &SqlitePool,
    identity_id: &str,
) -> Result<Vec<EndpointResponse>, StorageError> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM identity_endpoint_configs WHERE identity_id = ? ORDER BY created_at",
    )
    .bind(identity_id)
    .fetch_all(pool)
    .await?;
    let mut endpoints = Vec::with_capacity(ids.len());
    for id in ids {
        endpoints.push(get(pool, identity_id, &id).await?);
    }
    Ok(endpoints)
}

pub(crate) async fn get(
    pool: &SqlitePool,
    identity_id: &str,
    endpoint_id: &str,
) -> Result<EndpointResponse, StorageError> {
    let endpoint = sqlx::query_as::<_, EndpointRow>(
        "SELECT id, name, protocol, token, created_at FROM identity_endpoint_configs \
         WHERE id = ? AND identity_id = ?",
    )
    .bind(endpoint_id)
    .bind(identity_id)
    .fetch_optional(pool)
    .await?
    .ok_or(StorageError::EndpointNotFound)?;
    let models = sqlx::query_as::<_, EndpointModelRow>(
        "SELECT id, endpoint_id, model_name, provider_id, upstream_model, created_at \
         FROM identity_endpoint_models WHERE endpoint_id = ? ORDER BY candidate_order, id",
    )
    .bind(endpoint_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    Ok(EndpointResponse {
        id: endpoint.id,
        name: endpoint.name,
        protocol: endpoint.protocol,
        token: endpoint.token,
        models,
        created_at: endpoint.created_at,
    })
}

pub(crate) async fn update(
    pool: &SqlitePool,
    identity_id: &str,
    endpoint_id: &str,
    input: UpdateEndpointRequest,
) -> Result<EndpointResponse, StorageError> {
    let current = get(pool, identity_id, endpoint_id).await?;
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "UPDATE identity_endpoint_configs SET name = ?, protocol = ? WHERE id = ? AND identity_id = ?",
    )
    .bind(input.name.as_deref().map(str::trim).unwrap_or(&current.name))
    .bind(
        input
            .protocol
            .as_deref()
            .map(str::trim)
            .unwrap_or(&current.protocol),
    )
    .bind(endpoint_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    if let Some(models) = input.models {
        replace_models(
            &mut transaction,
            identity_id,
            endpoint_id,
            models,
            &Utc::now().to_rfc3339(),
        )
        .await?;
    }
    transaction.commit().await?;
    get(pool, identity_id, endpoint_id).await
}

pub(crate) async fn delete(
    pool: &SqlitePool,
    identity_id: &str,
    endpoint_id: &str,
) -> Result<(), StorageError> {
    let mut transaction = pool.begin().await?;
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM identity_endpoint_configs WHERE id = ? AND identity_id = ?)",
    )
    .bind(endpoint_id)
    .bind(identity_id)
    .fetch_one(&mut *transaction)
    .await?;
    if exists == 0 {
        return Err(StorageError::EndpointNotFound);
    }
    sqlx::query(
        "DELETE FROM identity_endpoint_models WHERE endpoint_id = ? \
         AND EXISTS (SELECT 1 FROM identity_endpoint_configs WHERE id = ? AND identity_id = ?)",
    )
    .bind(endpoint_id)
    .bind(endpoint_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM identity_endpoint_configs WHERE id = ? AND identity_id = ?")
        .bind(endpoint_id)
        .bind(identity_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn regenerate_token(
    pool: &SqlitePool,
    identity_id: &str,
    endpoint_id: &str,
) -> Result<EndpointResponse, StorageError> {
    let result = sqlx::query(
        "UPDATE identity_endpoint_configs SET token = ? WHERE id = ? AND identity_id = ?",
    )
    .bind(generate_credential())
    .bind(endpoint_id)
    .bind(identity_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(StorageError::EndpointNotFound);
    }
    get(pool, identity_id, endpoint_id).await
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
    endpoint_id: &str,
    models: Vec<EndpointModelInput>,
    created_at: &str,
) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM identity_endpoint_models WHERE endpoint_id = ?")
        .bind(endpoint_id)
        .execute(&mut **transaction)
        .await?;
    for (candidate_order, model) in models.into_iter().enumerate() {
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
            "INSERT INTO identity_endpoint_models \
             (id, endpoint_id, model_name, provider_id, upstream_model, candidate_order, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(endpoint_id)
        .bind(model_name)
        .bind(&model.provider_id)
        .bind(upstream_model)
        .bind(candidate_order as i64)
        .bind(created_at)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct EndpointRow {
    id: String,
    name: String,
    protocol: String,
    token: String,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct EndpointModelRow {
    id: String,
    endpoint_id: String,
    model_name: String,
    provider_id: String,
    upstream_model: String,
    created_at: String,
}

impl From<EndpointModelRow> for EndpointModelResponse {
    fn from(value: EndpointModelRow) -> Self {
        Self {
            id: value.id,
            endpoint_id: value.endpoint_id,
            model_name: value.model_name,
            provider_id: value.provider_id,
            upstream_model: value.upstream_model,
            created_at: value.created_at,
        }
    }
}
