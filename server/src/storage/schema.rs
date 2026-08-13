use sqlx::SqlitePool;

use super::StorageError;

pub(crate) async fn initialize(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS identities (\
            id TEXT PRIMARY KEY,\
            machine_id TEXT NOT NULL,\
            account_sid TEXT NOT NULL,\
            credential_hash TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            last_active_at TEXT NOT NULL,\
            UNIQUE(machine_id, account_sid)\
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS identity_provider_configs (\
            id TEXT PRIMARY KEY,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            name TEXT NOT NULL,\
            provider_type TEXT NOT NULL,\
            base_url TEXT NOT NULL,\
            api_key_ciphertext TEXT NOT NULL,\
            capabilities_json TEXT,\
            created_at TEXT NOT NULL\
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS identity_provider_models (\
            id TEXT PRIMARY KEY,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            model_name TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            UNIQUE(provider_id, model_name)\
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS identity_interface_configs (\
            id TEXT PRIMARY KEY,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            name TEXT NOT NULL,\
            protocol TEXT NOT NULL,\
            token TEXT NOT NULL UNIQUE,\
            created_at TEXT NOT NULL\
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS identity_interface_models (\
            id TEXT PRIMARY KEY,\
            interface_id TEXT NOT NULL REFERENCES identity_interface_configs(id),\
            model_name TEXT NOT NULL,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            upstream_model TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            UNIQUE(interface_id, model_name)\
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS identity_response_sessions (\
            response_id TEXT NOT NULL,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            previous_response_id TEXT,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            model TEXT NOT NULL,\
            input_messages_json TEXT NOT NULL,\
            output_items_json TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            PRIMARY KEY(response_id, identity_id)\
        )",
    )
    .execute(pool)
    .await?;
    upgrade_response_sessions_primary_key(pool).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS identity_request_logs (\
            id TEXT PRIMARY KEY,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            created_at TEXT NOT NULL,\
            protocol_in TEXT, protocol_out TEXT, protocol_upstream TEXT,\
            provider_id TEXT, provider_name TEXT, model_requested TEXT, model_upstream TEXT,\
            proxy_token_id TEXT, status TEXT NOT NULL, http_status INTEGER, error_code TEXT,\
            error_message TEXT, is_streaming INTEGER, input_tokens INTEGER, output_tokens INTEGER,\
            reasoning_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER,\
            estimated_cost REAL, currency TEXT, latency_ms INTEGER, upstream_latency_ms INTEGER,\
            first_token_ms INTEGER, tool_call_count INTEGER, upstream_request_id TEXT, metadata_json TEXT\
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS identity_model_aliases (\
            id TEXT PRIMARY KEY,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            alias TEXT NOT NULL,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            upstream_model TEXT NOT NULL,\
            downstream_protocols_json TEXT NOT NULL,\
            enabled INTEGER NOT NULL DEFAULT 1,\
            created_at TEXT NOT NULL,\
            UNIQUE(identity_id, alias)\
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn upgrade_response_sessions_primary_key(pool: &SqlitePool) -> Result<(), StorageError> {
    #[derive(sqlx::FromRow)]
    struct Column {
        name: String,
        pk: i64,
    }

    let columns = sqlx::query_as::<_, Column>("PRAGMA table_info(identity_response_sessions)")
        .fetch_all(pool)
        .await?;
    let has_legacy_primary_key = columns
        .iter()
        .any(|column| column.name == "response_id" && column.pk == 1)
        && !columns
            .iter()
            .any(|column| column.name == "identity_id" && column.pk == 2);
    if !has_legacy_primary_key {
        return Ok(());
    }

    let mut transaction = pool.begin().await?;
    sqlx::query(
        "CREATE TABLE identity_response_sessions_replacement (\
            response_id TEXT NOT NULL,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            previous_response_id TEXT,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            model TEXT NOT NULL,\
            input_messages_json TEXT NOT NULL,\
            output_items_json TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            PRIMARY KEY(response_id, identity_id)\
        )",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO identity_response_sessions_replacement (\
            response_id, identity_id, previous_response_id, provider_id, model, \
            input_messages_json, output_items_json, created_at\
        ) SELECT response_id, identity_id, previous_response_id, provider_id, model, \
            input_messages_json, output_items_json, created_at \
        FROM identity_response_sessions",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DROP TABLE identity_response_sessions")
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "ALTER TABLE identity_response_sessions_replacement \
         RENAME TO identity_response_sessions",
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}
