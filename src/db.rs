use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::ProviderConfig;

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
            created_at  TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_configs(pool: &SqlitePool) -> Result<Vec<ProviderConfig>> {
    let rows = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, created_at
         FROM provider_configs
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_config_by_id(pool: &SqlitePool, id: &str) -> Result<Option<ProviderConfig>> {
    let row = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, created_at
         FROM provider_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_config_by_token(pool: &SqlitePool, token: &str) -> Result<Option<ProviderConfig>> {
    let row = sqlx::query_as::<_, ProviderConfig>(
        "SELECT id, name, provider_type, base_url, api_key, token, created_at
         FROM provider_configs WHERE token = ?",
    )
    .bind(token)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create_config(
    pool: &SqlitePool,
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: &str,
) -> Result<ProviderConfig> {
    let id = Uuid::new_v4().to_string();
    let token = generate_token();
    let created_at = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO provider_configs (id, name, provider_type, base_url, api_key, token, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(provider_type)
    .bind(base_url)
    .bind(api_key)
    .bind(&token)
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
        created_at,
    })
}

pub async fn update_config(
    pool: &SqlitePool,
    id: &str,
    name: Option<&str>,
    provider_type: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> Result<bool> {
    let current = match get_config_by_id(pool, id).await? {
        Some(c) => c,
        None => return Ok(false),
    };

    let name = name.unwrap_or(&current.name);
    let provider_type = provider_type.unwrap_or(&current.provider_type);
    let base_url = base_url.unwrap_or(&current.base_url);
    let api_key = api_key.unwrap_or(&current.api_key);

    let result = sqlx::query(
        "UPDATE provider_configs SET name = ?, provider_type = ?, base_url = ?, api_key = ? WHERE id = ?",
    )
    .bind(name)
    .bind(provider_type)
    .bind(base_url)
    .bind(api_key)
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

fn generate_token() -> String {
    // Generate a 32-char hex token (128-bit randomness)
    Uuid::new_v4().to_string().replace('-', "")
}
