use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::{ModelAlias, ProviderCapabilityOverrides, ProviderConfig};

#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub provider: ProviderConfig,
    pub model_upstream: String,
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

    Ok(provider.map(|provider| ResolvedProvider {
        model_upstream: model.to_string(),
        provider,
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

pub async fn create_config_with_capabilities(
    pool: &SqlitePool,
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: &str,
    capabilities: Option<&ProviderCapabilityOverrides>,
) -> Result<ProviderConfig> {
    let id = Uuid::new_v4().to_string();
    let token = generate_token();
    let created_at = Utc::now().to_rfc3339();
    let capabilities_json = encode_capabilities(capabilities)?;

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
    .execute(pool)
    .await?;

    Ok(ProviderConfig {
        id,
        name: name.to_string(),
        provider_type: provider_type.to_string(),
        base_url: base_url.to_string(),
        api_key: api_key.to_string(),
        token,
        capabilities_json,
        created_at,
    })
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

pub async fn update_config_with_capabilities(
    pool: &SqlitePool,
    id: &str,
    name: Option<&str>,
    provider_type: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
    capabilities: Option<&ProviderCapabilityOverrides>,
) -> Result<bool> {
    let current = match get_config_by_id(pool, id).await? {
        Some(c) => c,
        None => return Ok(false),
    };

    let name = name.unwrap_or(&current.name);
    let provider_type = provider_type.unwrap_or(&current.provider_type);
    let base_url = base_url.unwrap_or(&current.base_url);
    let api_key = api_key.unwrap_or(&current.api_key);
    let capabilities_json = match capabilities {
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
    .bind(capabilities_json)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn delete_config(pool: &SqlitePool, id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM provider_configs WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
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

    use super::{create_config, create_model_alias, get_provider_by_model};

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
}
