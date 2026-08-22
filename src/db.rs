use anyhow::Result;
use chrono::Utc;
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::models::{
    EndpointConfig, EndpointModel, EndpointModelInput, ModelAlias, ProviderCapabilityOverrides,
    ProviderConfig, ProviderModel, UpdateConfigRequest,
};
#[cfg(test)]
use crate::providers::spec::ProviderSpec;
use crate::providers::spec::UpstreamProtocol;

#[derive(Debug, Clone)]
#[cfg(test)]
pub struct ResolvedProvider {
    pub provider: ProviderConfig,
    pub model_upstream: String,
}

pub struct ResolvedEndpointProvider {
    pub provider: ProviderConfig,
    pub model_upstream: String,
    pub upstream_protocol: UpstreamProtocol,
}

#[derive(Debug)]
pub enum EndpointWriteError {
    EndpointNotFound,
    ProviderModelNotFound {
        provider_id: String,
        upstream_model: String,
    },
    DuplicateModelName {
        model_name: String,
    },
    Storage(anyhow::Error),
}

impl std::fmt::Display for EndpointWriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EndpointNotFound => formatter.write_str("endpoint does not exist"),
            Self::ProviderModelNotFound {
                provider_id,
                upstream_model,
            } => write!(
                formatter,
                "provider model `{upstream_model}` does not exist for provider `{provider_id}`"
            ),
            Self::DuplicateModelName { model_name } => {
                write!(
                    formatter,
                    "endpoint model name `{model_name}` already exists"
                )
            }
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for EndpointWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for EndpointWriteError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(error.into())
    }
}

pub async fn init_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_configs (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            provider_type TEXT NOT NULL,
            base_url    TEXT NOT NULL,
            api_key     TEXT NOT NULL,
            token       TEXT NOT NULL UNIQUE,
            capabilities_json TEXT,
            created_at  TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;
    ensure_provider_capabilities_column(pool).await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_models (
            id          TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL,
            model_name  TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            UNIQUE(provider_id, model_name)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS request_logs (
            id                  TEXT PRIMARY KEY,
            created_at          TEXT NOT NULL,
            protocol_in         TEXT,
            protocol_out        TEXT,
            protocol_upstream   TEXT,
            provider_id         TEXT,
            provider_name       TEXT,
            model_requested     TEXT,
            model_upstream      TEXT,
            proxy_token_id      TEXT,
            status              TEXT NOT NULL,
            http_status         INTEGER,
            error_code          TEXT,
            error_message       TEXT,
            is_streaming        INTEGER,
            input_tokens        INTEGER,
            output_tokens       INTEGER,
            reasoning_tokens    INTEGER,
            cache_read_tokens   INTEGER,
            cache_write_tokens  INTEGER,
            estimated_cost      REAL,
            currency            TEXT,
            latency_ms          INTEGER,
            upstream_latency_ms INTEGER,
            first_token_ms      INTEGER,
            tool_call_count     INTEGER,
            upstream_request_id TEXT,
            metadata_json       TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS response_sessions (
            response_id          TEXT PRIMARY KEY,
            previous_response_id TEXT,
            provider_id          TEXT NOT NULL,
            model                TEXT NOT NULL,
            input_messages_json  TEXT NOT NULL,
            output_items_json    TEXT NOT NULL,
            created_at           TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS endpoint_configs (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            protocol    TEXT NOT NULL,
            token       TEXT NOT NULL UNIQUE,
            created_at  TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS endpoint_models (
            id              TEXT PRIMARY KEY,
            endpoint_id    TEXT NOT NULL,
            model_name      TEXT NOT NULL,
            provider_id     TEXT NOT NULL,
            upstream_model  TEXT NOT NULL,
            created_at      TEXT NOT NULL,
            UNIQUE(endpoint_id, model_name)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS model_aliases (
            id                          TEXT PRIMARY KEY,
            alias                       TEXT NOT NULL UNIQUE,
            provider_id                 TEXT NOT NULL,
            upstream_model              TEXT NOT NULL,
            downstream_protocols_json   TEXT NOT NULL,
            enabled                     INTEGER NOT NULL DEFAULT 1,
            created_at                  TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_endpoints(pool: &SqlitePool) -> Result<Vec<EndpointConfig>> {
    sqlx::query_as::<_, EndpointConfig>(
        "SELECT id, name, protocol, token, created_at FROM endpoint_configs ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn list_endpoint_models(pool: &SqlitePool) -> Result<Vec<EndpointModel>> {
    sqlx::query_as::<_, EndpointModel>(
        "SELECT id, endpoint_id, model_name, provider_id, upstream_model, created_at FROM endpoint_models ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn list_endpoint_models_by_interface(
    pool: &SqlitePool,
    endpoint_id: &str,
) -> Result<Vec<EndpointModel>> {
    sqlx::query_as::<_, EndpointModel>(
        "SELECT id, endpoint_id, model_name, provider_id, upstream_model, created_at
         FROM endpoint_models WHERE endpoint_id = ? ORDER BY created_at DESC",
    )
    .bind(endpoint_id)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn list_provider_models(pool: &SqlitePool) -> Result<Vec<ProviderModel>> {
    sqlx::query_as::<_, ProviderModel>(
        "SELECT id, provider_id, model_name, created_at FROM provider_models ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn list_provider_models_by_provider(
    pool: &SqlitePool,
    provider_id: &str,
) -> Result<Vec<ProviderModel>> {
    sqlx::query_as::<_, ProviderModel>(
        "SELECT id, provider_id, model_name, created_at
         FROM provider_models WHERE provider_id = ? ORDER BY created_at DESC",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn create_provider_model(
    pool: &SqlitePool,
    provider_id: &str,
    model_name: &str,
) -> Result<ProviderModel> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let model_name = model_name.trim();
    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, model_name, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(provider_id)
    .bind(model_name)
    .bind(&created_at)
    .execute(pool)
    .await?;

    Ok(ProviderModel {
        id,
        provider_id: provider_id.to_string(),
        model_name: model_name.to_string(),
        created_at,
    })
}

async fn insert_provider_model_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    provider_id: &str,
    normalized_model_name: &str,
) -> Result<ProviderModel> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, model_name, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(provider_id)
    .bind(normalized_model_name)
    .bind(&created_at)
    .execute(&mut **tx)
    .await?;

    Ok(ProviderModel {
        id,
        provider_id: provider_id.to_string(),
        model_name: normalized_model_name.to_string(),
        created_at,
    })
}

pub async fn upsert_provider_models(
    pool: &SqlitePool,
    provider_id: &str,
    model_names: &[String],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    for model_name in model_names {
        let model_name = model_name.trim();
        if model_name.is_empty() {
            continue;
        }
        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT OR IGNORE INTO provider_models (id, provider_id, model_name, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(provider_id)
        .bind(model_name)
        .bind(&created_at)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn delete_provider_model(
    pool: &SqlitePool,
    provider_id: &str,
    model_id: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let model_name = sqlx::query_scalar::<_, String>(
        "SELECT model_name FROM provider_models WHERE id = ? AND provider_id = ?",
    )
    .bind(model_id)
    .bind(provider_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(model_name) = model_name else {
        return Ok(false);
    };
    let deleted = delete_provider_model_in_tx(&mut tx, provider_id, &model_name).await?;
    tx.commit().await?;
    Ok(deleted)
}

async fn delete_provider_model_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    provider_id: &str,
    model_name: &str,
) -> Result<bool> {
    sqlx::query("DELETE FROM endpoint_models WHERE provider_id = ? AND upstream_model = ?")
        .bind(provider_id)
        .bind(model_name)
        .execute(&mut **tx)
        .await?;
    let result =
        sqlx::query("DELETE FROM provider_models WHERE provider_id = ? AND model_name = ?")
            .bind(provider_id)
            .bind(model_name)
            .execute(&mut **tx)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn provider_model_exists(
    pool: &SqlitePool,
    provider_id: &str,
    model_name: &str,
) -> Result<bool> {
    let exists: (i64,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM provider_models WHERE provider_id = ? AND model_name = ?)",
    )
    .bind(provider_id)
    .bind(model_name)
    .fetch_one(pool)
    .await?;
    Ok(exists.0 != 0)
}

pub async fn get_endpoint_by_token(
    pool: &SqlitePool,
    token: &str,
) -> Result<Option<EndpointConfig>> {
    sqlx::query_as::<_, EndpointConfig>(
        "SELECT id, name, protocol, token, created_at FROM endpoint_configs WHERE token = ?",
    )
    .bind(token)
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

#[cfg(test)]
pub async fn get_endpoint_by_id(pool: &SqlitePool, id: &str) -> Result<Option<EndpointConfig>> {
    sqlx::query_as::<_, EndpointConfig>(
        "SELECT id, name, protocol, token, created_at FROM endpoint_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

#[cfg(test)]
pub async fn create_interface(
    pool: &SqlitePool,
    name: &str,
    protocol: &str,
) -> Result<EndpointConfig> {
    let id = Uuid::new_v4().to_string();
    let token = generate_token();
    let created_at = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO endpoint_configs (id, name, protocol, token, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(protocol)
    .bind(&token)
    .bind(&created_at)
    .execute(pool)
    .await?;

    Ok(EndpointConfig {
        id,
        name: name.to_string(),
        protocol: protocol.to_string(),
        token,
        created_at,
    })
}

pub async fn create_endpoint_with_models(
    pool: &SqlitePool,
    name: &str,
    protocol: &str,
    inputs: &[EndpointModelInput],
) -> std::result::Result<(EndpointConfig, Vec<EndpointModel>), EndpointWriteError> {
    let inputs = inputs
        .iter()
        .map(normalize_endpoint_model_input)
        .collect::<Vec<_>>();
    let id = Uuid::new_v4().to_string();
    let token = generate_token();
    let created_at = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO endpoint_configs (id, name, protocol, token, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(protocol)
    .bind(&token)
    .bind(&created_at)
    .execute(&mut *tx)
    .await?;

    let mut models = Vec::with_capacity(inputs.len());
    for input in &inputs {
        validate_endpoint_model_reference_in_tx(&mut tx, input).await?;
        models.push(insert_endpoint_model_in_tx(&mut tx, &id, input).await?);
    }
    tx.commit().await?;

    Ok((
        EndpointConfig {
            id,
            name: name.to_string(),
            protocol: protocol.to_string(),
            token,
            created_at,
        },
        models,
    ))
}

pub async fn update_endpoint_with_models(
    pool: &SqlitePool,
    id: &str,
    name: Option<&str>,
    inputs: Option<&[EndpointModelInput]>,
) -> std::result::Result<(EndpointConfig, Vec<EndpointModel>), EndpointWriteError> {
    let inputs = inputs.map(|inputs| {
        inputs
            .iter()
            .map(normalize_endpoint_model_input)
            .collect::<Vec<_>>()
    });
    let mut tx = pool.begin().await?;
    let current = sqlx::query_as::<_, EndpointConfig>(
        "SELECT id, name, protocol, token, created_at FROM endpoint_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(EndpointWriteError::EndpointNotFound)?;
    let name = name.unwrap_or(&current.name);
    sqlx::query("UPDATE endpoint_configs SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(&mut *tx)
        .await?;

    if let Some(inputs) = inputs.as_deref() {
        for input in inputs {
            validate_endpoint_model_reference_in_tx(&mut tx, input).await?;
        }

        let existing_models = sqlx::query_as::<_, EndpointModel>(
            "SELECT id, endpoint_id, model_name, provider_id, upstream_model, created_at
             FROM endpoint_models WHERE endpoint_id = ?",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;

        for existing in &existing_models {
            let replacement = inputs.iter().find(|input| {
                input
                    .model_name
                    .as_deref()
                    .expect("normalized endpoint model name must be present")
                    == existing.model_name
            });
            match replacement {
                Some(input)
                    if input.provider_id != existing.provider_id
                        || input.upstream_model != existing.upstream_model =>
                {
                    sqlx::query(
                        "UPDATE endpoint_models SET provider_id = ?, upstream_model = ? WHERE id = ?",
                    )
                    .bind(&input.provider_id)
                    .bind(&input.upstream_model)
                    .bind(&existing.id)
                    .execute(&mut *tx)
                    .await?;
                }
                Some(_) => {}
                None => {
                    sqlx::query("DELETE FROM endpoint_models WHERE id = ?")
                        .bind(&existing.id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
        }

        for input in inputs {
            let model_name = input
                .model_name
                .as_deref()
                .expect("normalized endpoint model name must be present");
            if !existing_models
                .iter()
                .any(|existing| existing.model_name == model_name)
            {
                insert_endpoint_model_in_tx(&mut tx, id, input).await?;
            }
        }
    }

    let endpoint = sqlx::query_as::<_, EndpointConfig>(
        "SELECT id, name, protocol, token, created_at FROM endpoint_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let models = sqlx::query_as::<_, EndpointModel>(
        "SELECT id, endpoint_id, model_name, provider_id, upstream_model, created_at
         FROM endpoint_models WHERE endpoint_id = ? ORDER BY created_at DESC",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((endpoint, models))
}

async fn validate_endpoint_model_reference_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: &EndpointModelInput,
) -> std::result::Result<(), EndpointWriteError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(
            SELECT 1
            FROM provider_configs AS provider
            INNER JOIN provider_models AS model ON model.provider_id = provider.id
            WHERE provider.id = ? AND model.model_name = ?
        )",
    )
    .bind(&input.provider_id)
    .bind(&input.upstream_model)
    .fetch_one(&mut **tx)
    .await?;
    if exists == 0 {
        return Err(EndpointWriteError::ProviderModelNotFound {
            provider_id: input.provider_id.clone(),
            upstream_model: input.upstream_model.clone(),
        });
    }
    Ok(())
}

async fn insert_endpoint_model_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    endpoint_id: &str,
    input: &EndpointModelInput,
) -> std::result::Result<EndpointModel, EndpointWriteError> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let model_name = input
        .model_name
        .as_deref()
        .expect("normalized endpoint model name must be present");
    let result = sqlx::query(
        "INSERT INTO endpoint_models (id, endpoint_id, model_name, provider_id, upstream_model, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(endpoint_id)
    .bind(model_name)
    .bind(&input.provider_id)
    .bind(&input.upstream_model)
    .bind(&created_at)
    .execute(&mut **tx)
    .await;
    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            return Err(EndpointWriteError::DuplicateModelName {
                model_name: model_name.to_string(),
            });
        }
        Err(error) => return Err(EndpointWriteError::Storage(error.into())),
    }

    Ok(EndpointModel {
        id,
        endpoint_id: endpoint_id.to_string(),
        model_name: model_name.to_string(),
        provider_id: input.provider_id.clone(),
        upstream_model: input.upstream_model.clone(),
        created_at,
    })
}

fn normalize_endpoint_model_input(input: &EndpointModelInput) -> EndpointModelInput {
    let provider_id = input.provider_id.trim().to_string();
    let upstream_model = input.upstream_model.trim().to_string();
    let model_name = input
        .model_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&upstream_model)
        .to_string();
    EndpointModelInput {
        provider_id,
        upstream_model,
        model_name: Some(model_name),
    }
}

pub async fn delete_interface(pool: &SqlitePool, id: &str) -> Result<bool> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM endpoint_models WHERE endpoint_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("DELETE FROM endpoint_configs WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

pub async fn regenerate_endpoint_token(pool: &SqlitePool, id: &str) -> Result<Option<String>> {
    let new_token = generate_token();
    let result = sqlx::query("UPDATE endpoint_configs SET token = ? WHERE id = ?")
        .bind(&new_token)
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() > 0 {
        Ok(Some(new_token))
    } else {
        Ok(None)
    }
}

pub async fn create_endpoint_model(
    pool: &SqlitePool,
    endpoint_id: &str,
    model_name: &str,
    provider_id: &str,
    upstream_model: &str,
) -> std::result::Result<EndpointModel, EndpointWriteError> {
    let input = normalize_endpoint_model_input(&EndpointModelInput {
        provider_id: provider_id.to_string(),
        upstream_model: upstream_model.to_string(),
        model_name: Some(model_name.to_string()),
    });
    let mut tx = pool.begin().await?;
    let endpoint_exists =
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM endpoint_configs WHERE id = ?)")
            .bind(endpoint_id)
            .fetch_one(&mut *tx)
            .await?;
    if endpoint_exists == 0 {
        return Err(EndpointWriteError::EndpointNotFound);
    }

    validate_endpoint_model_reference_in_tx(&mut tx, &input).await?;

    let model = insert_endpoint_model_in_tx(&mut tx, endpoint_id, &input).await?;
    tx.commit().await?;
    Ok(model)
}

pub async fn delete_endpoint_model(
    pool: &SqlitePool,
    endpoint_id: &str,
    model_id: &str,
) -> Result<bool> {
    let result = sqlx::query("DELETE FROM endpoint_models WHERE endpoint_id = ? AND id = ?")
        .bind(endpoint_id)
        .bind(model_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_endpoint_model(
    pool: &SqlitePool,
    endpoint_id: &str,
    model_name: &str,
) -> Result<Option<EndpointModel>> {
    sqlx::query_as::<_, EndpointModel>(
        "SELECT id, endpoint_id, model_name, provider_id, upstream_model, created_at
         FROM endpoint_models WHERE endpoint_id = ? AND model_name = ?",
    )
    .bind(endpoint_id)
    .bind(model_name)
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

pub async fn list_configs(pool: &SqlitePool) -> Result<Vec<ProviderConfig>> {
    let rows = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, capabilities_json, created_at
         FROM provider_configs
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_model_aliases(pool: &SqlitePool) -> Result<Vec<ModelAlias>> {
    let rows = sqlx::query(
        r#"
        SELECT
            alias,
            provider_id,
            upstream_model,
            downstream_protocols_json
        FROM model_aliases
        WHERE enabled = 1
        ORDER BY alias ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            use sqlx::Row;

            ModelAlias {
                alias: row.get("alias"),
                provider_id: row.get("provider_id"),
                upstream_model: row.get("upstream_model"),
                downstream_protocols: decode_downstream_protocols(
                    row.get::<String, _>("downstream_protocols_json").as_str(),
                ),
            }
        })
        .collect())
}

pub async fn get_config_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ProviderConfig>> {
    let row = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, capabilities_json, created_at
         FROM provider_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_config_by_token(pool: &SqlitePool, token: &str) -> Result<Option<ProviderConfig>> {
    let row = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, capabilities_json, created_at
         FROM provider_configs WHERE token = ?",
    )
    .bind(token)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
pub async fn get_provider_by_model(
    pool: &SqlitePool,
    model: &str,
    downstream_protocol: &str,
) -> Result<Option<ResolvedProvider>> {
    if let Some(alias) = get_model_alias(pool, model).await? {
        if !alias_allows_protocol(&alias, downstream_protocol) {
            return Ok(None);
        }
        let Some(provider) = get_config_by_id(pool, &alias.provider_id).await? else {
            return Ok(None);
        };
        return Ok(Some(ResolvedProvider {
            provider,
            model_upstream: alias.upstream_model,
        }));
    }

    let provider = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, capabilities_json, created_at
         FROM provider_configs WHERE name = ?",
    )
    .bind(model)
    .fetch_optional(pool)
    .await?;

    Ok(provider.and_then(|provider| {
        let spec = ProviderSpec::from_provider_config(&provider);
        spec.supports_downstream(downstream_protocol)
            .then_some(ResolvedProvider {
                model_upstream: model.to_string(),
                provider,
            })
    }))
}

pub async fn get_model_alias(pool: &SqlitePool, alias: &str) -> Result<Option<ModelAlias>> {
    let row = sqlx::query(
        r#"
        SELECT
            id,
            alias,
            provider_id,
            upstream_model,
            downstream_protocols_json,
            enabled,
            created_at
        FROM model_aliases
        WHERE alias = ? AND enabled = 1
        "#,
    )
    .bind(alias)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| {
        use sqlx::Row;

        ModelAlias {
            alias: row.get("alias"),
            provider_id: row.get("provider_id"),
            upstream_model: row.get("upstream_model"),
            downstream_protocols: decode_downstream_protocols(
                row.get::<String, _>("downstream_protocols_json").as_str(),
            ),
        }
    }))
}

#[cfg(test)]
fn alias_allows_protocol(alias: &ModelAlias, downstream_protocol: &str) -> bool {
    alias
        .downstream_protocols
        .iter()
        .any(|protocol| protocol == downstream_protocol)
}

fn decode_downstream_protocols(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

#[cfg(test)]
pub async fn create_config(
    pool: &SqlitePool,
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: &str,
) -> Result<ProviderConfig> {
    create_config_with_capabilities(pool, name, provider_type, base_url, api_key, None).await
}

#[cfg(test)]
pub async fn create_config_with_capabilities(
    pool: &SqlitePool,
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: &str,
    capabilities: Option<&ProviderCapabilityOverrides>,
) -> Result<ProviderConfig> {
    let (config, _) = create_config_with_models(
        pool,
        name,
        provider_type,
        base_url,
        api_key,
        capabilities,
        &[],
    )
    .await?;
    Ok(config)
}

pub async fn create_config_with_models(
    pool: &SqlitePool,
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: &str,
    capabilities: Option<&ProviderCapabilityOverrides>,
    model_names: &[String],
) -> Result<(ProviderConfig, Vec<ProviderModel>)> {
    let id = Uuid::new_v4().to_string();
    let token = generate_token();
    let created_at = Utc::now().to_rfc3339();
    let capabilities_json = encode_capabilities(capabilities)?;
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO provider_configs (id, name, provider_type, base_url, api_key, token, capabilities_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(provider_type)
    .bind(base_url)
    .bind(api_key)
    .bind(&token)
    .bind(&capabilities_json)
    .bind(&created_at)
    .execute(&mut *tx)
    .await?;

    let mut models = Vec::with_capacity(model_names.len());
    for model_name in model_names {
        let model_name = model_name.trim();
        models.push(insert_provider_model_in_tx(&mut tx, &id, model_name).await?);
    }

    tx.commit().await?;
    Ok((
        ProviderConfig {
            id,
            name: name.to_string(),
            provider_type: provider_type.to_string(),
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            token,
            capabilities_json,
            created_at,
        },
        models,
    ))
}

pub async fn create_model_alias(
    pool: &SqlitePool,
    alias: &str,
    provider_id: &str,
    upstream_model: &str,
    downstream_protocols: &[&str],
) -> Result<ModelAlias> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let downstream_protocols_json = serde_json::to_string(downstream_protocols)?;

    sqlx::query(
        r#"
        INSERT INTO model_aliases (
            id,
            alias,
            provider_id,
            upstream_model,
            downstream_protocols_json,
            enabled,
            created_at
        )
        VALUES (?, ?, ?, ?, ?, 1, ?)
        "#,
    )
    .bind(&id)
    .bind(alias)
    .bind(provider_id)
    .bind(upstream_model)
    .bind(&downstream_protocols_json)
    .bind(&created_at)
    .execute(pool)
    .await?;

    Ok(ModelAlias {
        alias: alias.to_string(),
        provider_id: provider_id.to_string(),
        upstream_model: upstream_model.to_string(),
        downstream_protocols: downstream_protocols
            .iter()
            .map(|protocol| (*protocol).to_string())
            .collect(),
    })
}

pub async fn delete_model_alias_protocol(
    pool: &SqlitePool,
    alias: &str,
    downstream_protocol: &str,
) -> Result<bool> {
    let Some(current) = get_model_alias(pool, alias).await? else {
        return Ok(false);
    };
    if !current
        .downstream_protocols
        .iter()
        .any(|protocol| protocol == downstream_protocol)
    {
        return Ok(false);
    }

    let remaining_protocols = current
        .downstream_protocols
        .into_iter()
        .filter(|protocol| protocol != downstream_protocol)
        .collect::<Vec<_>>();
    if remaining_protocols.is_empty() {
        let result = sqlx::query("UPDATE model_aliases SET enabled = 0 WHERE alias = ?")
            .bind(alias)
            .execute(pool)
            .await?;
        return Ok(result.rows_affected() > 0);
    }

    let downstream_protocols_json = serde_json::to_string(&remaining_protocols)?;
    let result =
        sqlx::query("UPDATE model_aliases SET downstream_protocols_json = ? WHERE alias = ?")
            .bind(&downstream_protocols_json)
            .bind(alias)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update_config_with_models(
    pool: &SqlitePool,
    id: &str,
    update: &UpdateConfigRequest,
    model_names: Option<&[String]>,
) -> Result<Option<(ProviderConfig, Vec<ProviderModel>)>> {
    let mut tx = pool.begin().await?;
    let current = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, capabilities_json, created_at
         FROM provider_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(current) = current else {
        return Ok(None);
    };

    let name = update.name.as_deref().unwrap_or(&current.name);
    let provider_type = update
        .provider_type
        .as_deref()
        .unwrap_or(&current.provider_type);
    let base_url = update.base_url.as_deref().unwrap_or(&current.base_url);
    let api_key = update.api_key.as_deref().unwrap_or(&current.api_key);
    let capabilities_json = match update.capabilities.as_ref() {
        Some(capabilities) => encode_capabilities(Some(capabilities))?,
        None => current.capabilities_json,
    };

    let result = sqlx::query(
        "UPDATE provider_configs SET name = ?, provider_type = ?, base_url = ?, api_key = ?, capabilities_json = ? WHERE id = ?",
    )
    .bind(name)
    .bind(provider_type)
    .bind(base_url)
    .bind(api_key)
    .bind(&capabilities_json)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    let existing_models = sqlx::query_as::<_, ProviderModel>(
        "SELECT id, provider_id, model_name, created_at
         FROM provider_models WHERE provider_id = ? ORDER BY created_at DESC",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;

    if let Some(model_names) = model_names {
        for existing in &existing_models {
            if model_names
                .iter()
                .all(|model_name| model_name.trim() != existing.model_name)
            {
                delete_provider_model_in_tx(&mut tx, id, &existing.model_name).await?;
            }
        }

        for model_name in model_names {
            let model_name = model_name.trim();
            if existing_models
                .iter()
                .any(|existing| existing.model_name == model_name)
            {
                continue;
            }
            insert_provider_model_in_tx(&mut tx, id, model_name).await?;
        }
    }

    let config = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, capabilities_json, created_at
         FROM provider_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let models = sqlx::query_as::<_, ProviderModel>(
        "SELECT id, provider_id, model_name, created_at
         FROM provider_models WHERE provider_id = ? ORDER BY created_at DESC",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Some((config, models)))
}

pub async fn delete_config(pool: &SqlitePool, id: &str) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let exists = sqlx::query_scalar::<_, i64>("SELECT 1 FROM provider_configs WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .is_some();
    if !exists {
        return Ok(false);
    }

    sqlx::query("DELETE FROM endpoint_models WHERE provider_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM provider_models WHERE provider_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM model_aliases WHERE provider_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("DELETE FROM provider_configs WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

pub async fn regenerate_token(pool: &SqlitePool, id: &str) -> Result<Option<String>> {
    let new_token = generate_token();
    let result = sqlx::query("UPDATE provider_configs SET token = ? WHERE id = ?")
        .bind(&new_token)
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() > 0 {
        Ok(Some(new_token))
    } else {
        Ok(None)
    }
}

async fn ensure_provider_capabilities_column(pool: &SqlitePool) -> Result<()> {
    let columns = sqlx::query("PRAGMA table_info(provider_configs)")
        .fetch_all(pool)
        .await?;
    let has_capabilities = columns.iter().any(|row| {
        use sqlx::Row;
        row.get::<String, _>("name") == "capabilities_json"
    });
    if !has_capabilities {
        sqlx::query("ALTER TABLE provider_configs ADD COLUMN capabilities_json TEXT")
            .execute(pool)
            .await?;
    }
    Ok(())
}

fn encode_capabilities(
    capabilities: Option<&ProviderCapabilityOverrides>,
) -> Result<Option<String>> {
    let Some(capabilities) = capabilities else {
        return Ok(None);
    };
    Ok(Some(serde_json::to_string(capabilities)?))
}

fn generate_token() -> String {
    // Generate a 32-char hex token (128-bit randomness)
    Uuid::new_v4().to_string().replace('-', "")
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use crate::models::{EndpointModelInput, UpdateConfigRequest};

    use super::{
        create_config, create_endpoint_model, create_interface, create_model_alias,
        create_provider_model, get_provider_by_model, list_endpoint_models,
        list_provider_models_by_provider,
    };

    fn config_update(name: Option<&str>) -> UpdateConfigRequest {
        UpdateConfigRequest {
            name: name.map(str::to_string),
            provider_type: None,
            base_url: None,
            api_key: None,
            capabilities: None,
            models: None,
        }
    }

    fn endpoint_model_input(
        provider_id: &str,
        upstream_model: &str,
        model_name: Option<&str>,
    ) -> EndpointModelInput {
        EndpointModelInput {
            provider_id: provider_id.to_string(),
            upstream_model: upstream_model.to_string(),
            model_name: model_name.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn creates_endpoint_and_complete_model_mapping_in_one_transaction() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        create_provider_model(&db, &provider.id, "model-a")
            .await
            .expect("create provider model");
        let mappings = vec![endpoint_model_input(
            &provider.id,
            "model-a",
            Some("alias-a"),
        )];

        let (endpoint, models) =
            super::create_endpoint_with_models(&db, "Endpoint", "all", &mappings)
                .await
                .expect("create endpoint with models");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].endpoint_id, endpoint.id);
        assert_eq!(models[0].model_name, "alias-a");
        assert_eq!(models[0].provider_id, provider.id);
        assert_eq!(models[0].upstream_model, "model-a");
    }

    #[tokio::test]
    async fn update_endpoint_replaces_complete_model_mapping() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        for model_name in ["model-a", "model-b"] {
            create_provider_model(&db, &provider.id, model_name)
                .await
                .expect("create provider model");
        }
        let original = vec![endpoint_model_input(
            &provider.id,
            "model-a",
            Some("old-alias"),
        )];
        let (endpoint, _) = super::create_endpoint_with_models(&db, "Original", "all", &original)
            .await
            .expect("create endpoint");
        let replacement = vec![endpoint_model_input(
            &provider.id,
            "model-b",
            Some("new-alias"),
        )];

        let (updated, models) = super::update_endpoint_with_models(
            &db,
            &endpoint.id,
            Some("Updated"),
            Some(&replacement),
        )
        .await
        .expect("update endpoint");

        assert_eq!(updated.name, "Updated");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_name, "new-alias");
        assert_eq!(models[0].upstream_model, "model-b");
    }

    #[tokio::test]
    async fn update_endpoint_without_models_preserves_model_identity() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        create_provider_model(&db, &provider.id, "model-a")
            .await
            .expect("create provider model");
        let mappings = vec![endpoint_model_input(
            &provider.id,
            "model-a",
            Some("alias-a"),
        )];
        let (endpoint, original_models) =
            super::create_endpoint_with_models(&db, "Original", "all", &mappings)
                .await
                .expect("create endpoint");

        let (_, models) =
            super::update_endpoint_with_models(&db, &endpoint.id, Some("Updated"), None)
                .await
                .expect("update endpoint");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, original_models[0].id);
        assert_eq!(models[0].created_at, original_models[0].created_at);
    }

    #[tokio::test]
    async fn update_endpoint_with_unchanged_models_preserves_model_identity() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        create_provider_model(&db, &provider.id, "model-a")
            .await
            .expect("create provider model");
        let mappings = vec![endpoint_model_input(
            &provider.id,
            "model-a",
            Some("alias-a"),
        )];
        let (endpoint, original_models) =
            super::create_endpoint_with_models(&db, "Original", "all", &mappings)
                .await
                .expect("create endpoint");

        let (_, models) =
            super::update_endpoint_with_models(&db, &endpoint.id, Some("Updated"), Some(&mappings))
                .await
                .expect("update endpoint");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, original_models[0].id);
        assert_eq!(models[0].created_at, original_models[0].created_at);
    }

    #[tokio::test]
    async fn update_endpoint_with_whitespace_normalized_models_preserves_model_identity() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        create_provider_model(&db, &provider.id, "model-a")
            .await
            .expect("create provider model");
        let mappings = vec![endpoint_model_input(
            &provider.id,
            "model-a",
            Some("alias-a"),
        )];
        let (endpoint, original_models) =
            super::create_endpoint_with_models(&db, "Original", "all", &mappings)
                .await
                .expect("create endpoint");
        let whitespace_padded = vec![endpoint_model_input(
            &format!(" {} ", provider.id),
            " model-a ",
            Some(" alias-a "),
        )];

        let (_, models) =
            super::update_endpoint_with_models(&db, &endpoint.id, None, Some(&whitespace_padded))
                .await
                .expect("update endpoint");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, original_models[0].id);
        assert_eq!(models[0].created_at, original_models[0].created_at);
    }

    #[tokio::test]
    async fn duplicate_insert_helper_reports_duplicate_model_name() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let endpoint = create_interface(&db, "Endpoint", "all")
            .await
            .expect("create endpoint");
        let input = endpoint_model_input("provider", "model-a", Some("alias-a"));
        let mut tx = db.begin().await.expect("begin transaction");
        super::insert_endpoint_model_in_tx(&mut tx, &endpoint.id, &input)
            .await
            .expect("insert first endpoint model");

        let result = super::insert_endpoint_model_in_tx(&mut tx, &endpoint.id, &input).await;

        assert!(matches!(
            result,
            Err(super::EndpointWriteError::DuplicateModelName { ref model_name })
                if model_name == "alias-a"
        ));
    }

    #[tokio::test]
    async fn update_endpoint_model_mapping_preserves_identity_for_same_model_name() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        for model_name in ["model-a", "model-b"] {
            create_provider_model(&db, &provider.id, model_name)
                .await
                .expect("create provider model");
        }
        let original = vec![endpoint_model_input(
            &provider.id,
            "model-a",
            Some("alias-a"),
        )];
        let (endpoint, original_models) =
            super::create_endpoint_with_models(&db, "Original", "all", &original)
                .await
                .expect("create endpoint");
        let replacement = vec![endpoint_model_input(
            &provider.id,
            "model-b",
            Some("alias-a"),
        )];

        let (_, models) =
            super::update_endpoint_with_models(&db, &endpoint.id, None, Some(&replacement))
                .await
                .expect("update endpoint");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, original_models[0].id);
        assert_eq!(models[0].created_at, original_models[0].created_at);
        assert_eq!(models[0].upstream_model, "model-b");
    }

    #[tokio::test]
    async fn invalid_provider_model_rolls_back_endpoint_creation() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        let mappings = vec![endpoint_model_input(
            &provider.id,
            "missing-model",
            Some("alias-a"),
        )];

        let result =
            super::create_endpoint_with_models(&db, "Must Roll Back", "all", &mappings).await;

        assert!(matches!(
            result,
            Err(super::EndpointWriteError::ProviderModelNotFound { .. })
        ));
        assert!(super::list_endpoints(&db)
            .await
            .expect("list endpoints")
            .is_empty());
        assert!(list_endpoint_models(&db)
            .await
            .expect("list endpoint models")
            .is_empty());
    }

    #[tokio::test]
    async fn invalid_provider_model_rolls_back_endpoint_update() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
        )
        .await
        .expect("create provider");
        create_provider_model(&db, &provider.id, "model-a")
            .await
            .expect("create provider model");
        let original = vec![endpoint_model_input(
            &provider.id,
            "model-a",
            Some("old-alias"),
        )];
        let (endpoint, original_models) =
            super::create_endpoint_with_models(&db, "Original", "all", &original)
                .await
                .expect("create endpoint");
        let invalid = vec![endpoint_model_input(
            &provider.id,
            "missing-model",
            Some("new-alias"),
        )];

        let result = super::update_endpoint_with_models(
            &db,
            &endpoint.id,
            Some("Must Roll Back"),
            Some(&invalid),
        )
        .await;

        assert!(matches!(
            result,
            Err(super::EndpointWriteError::ProviderModelNotFound { .. })
        ));
        let stored = super::get_endpoint_by_id(&db, &endpoint.id)
            .await
            .expect("get endpoint")
            .expect("endpoint exists");
        assert_eq!(stored.name, "Original");
        let stored_models = super::list_endpoint_models_by_interface(&db, &endpoint.id)
            .await
            .expect("list endpoint models");
        assert_eq!(stored_models.len(), 1);
        assert_eq!(stored_models[0].id, original_models[0].id);
        assert_eq!(stored_models[0].model_name, "old-alias");
    }

    #[tokio::test]
    async fn creates_config_and_complete_model_set_in_one_transaction() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let model_names = vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()];

        let (provider, models) = super::create_config_with_models(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
            None,
            &model_names,
        )
        .await
        .expect("create provider with models");

        assert_eq!(
            models
                .iter()
                .map(|model| model.model_name.as_str())
                .collect::<Vec<_>>(),
            ["deepseek-chat", "deepseek-reasoner"]
        );
        let stored = list_provider_models_by_provider(&db, &provider.id)
            .await
            .expect("list provider models");
        assert_eq!(stored.len(), 2);
    }

    #[tokio::test]
    async fn rolls_back_config_and_models_when_model_insert_fails() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let duplicate_models = vec!["model-a".to_string(), "model-a".to_string()];

        let result = super::create_config_with_models(
            &db,
            "Provider",
            "openai_compatible",
            "https://provider.example",
            "sk-provider",
            None,
            &duplicate_models,
        )
        .await;

        assert!(result.is_err());
        assert!(super::list_configs(&db)
            .await
            .expect("list provider configs")
            .is_empty());
        assert!(super::list_provider_models(&db)
            .await
            .expect("list provider models")
            .is_empty());
    }

    #[tokio::test]
    async fn replaces_complete_model_set_and_removes_only_stale_endpoint_references() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let original_names = vec!["model-kept".to_string(), "model-removed".to_string()];
        let (provider, original_models) = super::create_config_with_models(
            &db,
            "Original Provider",
            "openai_compatible",
            "https://old.example",
            "sk-old",
            None,
            &original_names,
        )
        .await
        .expect("create provider with models");
        let kept_before = original_models
            .iter()
            .find(|model| model.model_name == "model-kept")
            .expect("kept model")
            .clone();
        let endpoint = create_interface(&db, "Responses", "responses")
            .await
            .expect("create endpoint");
        create_endpoint_model(&db, &endpoint.id, "kept-alias", &provider.id, "model-kept")
            .await
            .expect("create kept endpoint model");
        create_endpoint_model(
            &db,
            &endpoint.id,
            "removed-alias",
            &provider.id,
            "model-removed",
        )
        .await
        .expect("create removed endpoint model");
        let replacement = vec!["model-kept".to_string(), "model-new".to_string()];
        let update = config_update(Some("Updated Provider"));

        let (updated, models) =
            super::update_config_with_models(&db, &provider.id, &update, Some(&replacement))
                .await
                .expect("replace provider models")
                .expect("provider exists");

        assert_eq!(updated.name, "Updated Provider");
        assert_eq!(models.len(), 2);
        let kept_after = models
            .iter()
            .find(|model| model.model_name == "model-kept")
            .expect("retained model");
        assert_eq!(kept_after.id, kept_before.id);
        assert_eq!(kept_after.created_at, kept_before.created_at);
        assert!(models.iter().any(|model| model.model_name == "model-new"));
        assert!(!models
            .iter()
            .any(|model| model.model_name == "model-removed"));

        let endpoint_models = list_endpoint_models(&db)
            .await
            .expect("list endpoint models");
        assert_eq!(endpoint_models.len(), 1);
        assert_eq!(endpoint_models[0].upstream_model, "model-kept");
    }

    #[tokio::test]
    async fn updating_config_without_models_preserves_complete_model_set() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let model_names = vec!["model-a".to_string(), "model-b".to_string()];
        let (provider, original_models) = super::create_config_with_models(
            &db,
            "Original Provider",
            "openai_compatible",
            "https://old.example",
            "sk-old",
            None,
            &model_names,
        )
        .await
        .expect("create provider with models");
        let update = config_update(Some("Updated Provider"));

        let (_, models) = super::update_config_with_models(&db, &provider.id, &update, None)
            .await
            .expect("update provider")
            .expect("provider exists");

        assert_eq!(models.len(), original_models.len());
        for original in original_models {
            let preserved = models
                .iter()
                .find(|model| model.model_name == original.model_name)
                .expect("preserved model");
            assert_eq!(preserved.id, original.id);
            assert_eq!(preserved.created_at, original.created_at);
        }
    }

    #[tokio::test]
    async fn failed_model_replacement_rolls_back_provider_and_old_models() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let original_names = vec!["model-old".to_string()];
        let (provider, original_models) = super::create_config_with_models(
            &db,
            "Original Provider",
            "openai_compatible",
            "https://old.example",
            "sk-old",
            None,
            &original_names,
        )
        .await
        .expect("create provider with models");
        let invalid_replacement = vec!["model-new".to_string(), "model-new".to_string()];
        let update = config_update(Some("Must Roll Back"));

        let result = super::update_config_with_models(
            &db,
            &provider.id,
            &update,
            Some(&invalid_replacement),
        )
        .await;

        assert!(result.is_err());
        let stored_provider = super::get_config_by_id(&db, &provider.id)
            .await
            .expect("get provider")
            .expect("provider exists");
        assert_eq!(stored_provider.name, "Original Provider");
        let stored_models = list_provider_models_by_provider(&db, &provider.id)
            .await
            .expect("list provider models");
        assert_eq!(stored_models.len(), 1);
        assert_eq!(stored_models[0].id, original_models[0].id);
        assert_eq!(stored_models[0].model_name, "model-old");
    }

    #[tokio::test]
    async fn resolves_model_alias_to_provider_and_upstream_model() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        create_model_alias(
            &db,
            "coder",
            &provider.id,
            "deepseek-chat",
            &["responses", "chat_completions"],
        )
        .await
        .expect("create alias");

        let resolved = get_provider_by_model(&db, "coder", "responses")
            .await
            .expect("resolve provider")
            .expect("provider found");

        assert_eq!(resolved.provider.id, provider.id);
        assert_eq!(resolved.model_upstream, "deepseek-chat");

        let blocked = get_provider_by_model(&db, "coder", "anthropic_messages")
            .await
            .expect("resolve provider");
        assert!(blocked.is_none());
    }

    #[tokio::test]
    async fn falls_back_to_provider_name_when_alias_is_missing() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "deepseek-chat",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");

        let resolved = get_provider_by_model(&db, "deepseek-chat", "responses")
            .await
            .expect("resolve provider")
            .expect("provider found");

        assert_eq!(resolved.provider.id, provider.id);
        assert_eq!(resolved.model_upstream, "deepseek-chat");
    }

    #[tokio::test]
    async fn does_not_fallback_to_provider_name_when_provider_protocol_is_not_allowed() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        create_config(
            &db,
            "OpenAI Responses",
            "openai",
            "https://api.openai.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");

        let resolved = get_provider_by_model(&db, "OpenAI Responses", "chat_completions")
            .await
            .expect("resolve provider");

        assert!(resolved.is_none());
    }

    #[tokio::test]
    async fn does_not_fallback_to_provider_name_when_alias_protocol_is_blocked() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "coder",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        create_model_alias(&db, "coder", &provider.id, "deepseek-chat", &["responses"])
            .await
            .expect("create alias");

        let resolved = get_provider_by_model(&db, "coder", "chat_completions")
            .await
            .expect("resolve provider");

        assert!(resolved.is_none());
    }

    #[tokio::test]
    async fn lists_models_added_to_provider() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");

        create_provider_model(&db, &provider.id, "deepseek-chat")
            .await
            .expect("create provider model");

        let models = list_provider_models_by_provider(&db, &provider.id)
            .await
            .expect("list provider models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].provider_id, provider.id);
        assert_eq!(models[0].model_name, "deepseek-chat");
    }

    #[tokio::test]
    async fn rejects_endpoint_model_outside_provider_model_catalog() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let endpoint = create_interface(&db, "Responses", "responses")
            .await
            .expect("create endpoint");

        let result = create_endpoint_model(
            &db,
            &endpoint.id,
            "not-in-catalog",
            &provider.id,
            "not-in-catalog",
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn deleting_provider_removes_provider_and_endpoint_models() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let provider = create_config(
            &db,
            "DeepSeek Provider",
            "openai_compatible",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        create_provider_model(&db, &provider.id, "deepseek-chat")
            .await
            .expect("create provider model");
        let endpoint = create_interface(&db, "Responses", "responses")
            .await
            .expect("create endpoint");
        create_endpoint_model(&db, &endpoint.id, "coder", &provider.id, "deepseek-chat")
            .await
            .expect("create endpoint model");

        let deleted = super::delete_config(&db, &provider.id)
            .await
            .expect("delete provider");

        assert!(deleted);
        assert!(list_provider_models_by_provider(&db, &provider.id)
            .await
            .expect("list provider models")
            .is_empty());
        assert!(list_endpoint_models(&db)
            .await
            .expect("list endpoint models")
            .is_empty());
    }

    #[tokio::test]
    async fn deleting_missing_provider_preserves_orphaned_associations() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        super::init_schema(&db).await.expect("init schema");
        let missing_provider_id = "missing-provider";

        sqlx::query(
            "INSERT INTO provider_models (id, provider_id, model_name, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind("orphaned-provider-model")
        .bind(missing_provider_id)
        .bind("model-a")
        .bind("2026-08-01T00:00:00Z")
        .execute(&db)
        .await
        .expect("insert orphaned provider model");
        sqlx::query(
            "INSERT INTO endpoint_models (id, endpoint_id, model_name, provider_id, upstream_model, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind("orphaned-endpoint-model")
        .bind("missing-endpoint")
        .bind("model-a")
        .bind(missing_provider_id)
        .bind("model-a")
        .bind("2026-08-01T00:00:00Z")
        .execute(&db)
        .await
        .expect("insert orphaned endpoint model");
        sqlx::query(
            "INSERT INTO model_aliases (id, alias, provider_id, upstream_model, downstream_protocols_json, enabled, created_at) VALUES (?, ?, ?, ?, ?, 1, ?)",
        )
        .bind("orphaned-alias-id")
        .bind("orphaned-alias")
        .bind(missing_provider_id)
        .bind("model-a")
        .bind("[\"responses\"]")
        .bind("2026-08-01T00:00:00Z")
        .execute(&db)
        .await
        .expect("insert orphaned model alias");

        let deleted = super::delete_config(&db, missing_provider_id)
            .await
            .expect("delete missing provider");

        assert!(!deleted);
        for table in ["provider_models", "endpoint_models", "model_aliases"] {
            let count = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM {table} WHERE provider_id = ?"
            ))
            .bind(missing_provider_id)
            .fetch_one(&db)
            .await
            .expect("count orphaned associations");
            assert_eq!(count, 1, "{table} row should remain");
        }
    }
}
