use sqlx::{Sqlite, SqlitePool, Transaction};

use super::StorageError;

pub(crate) async fn initialize(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(pool)
        .await?;
    let mut transaction = pool.begin().await?;
    discard_unscoped_legacy_schema(&mut transaction).await?;
    migrate_interface_schema(&mut transaction).await?;
    create_identity_schema(&mut transaction).await?;
    upgrade_endpoint_model_candidates(&mut transaction).await?;
    upgrade_request_log_columns(&mut transaction).await?;
    upgrade_response_sessions_primary_key(&mut transaction).await?;
    transaction.commit().await?;
    Ok(())
}

async fn migrate_interface_schema(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StorageError> {
    for (legacy, current) in [
        ("identity_interface_configs", "identity_endpoint_configs"),
        ("identity_interface_models", "identity_endpoint_models"),
        (
            "identity_interface_model_routes",
            "identity_endpoint_model_routes",
        ),
    ] {
        if table_exists(transaction, legacy).await? && !table_exists(transaction, current).await? {
            sqlx::query(&format!("ALTER TABLE {legacy} RENAME TO {current}"))
                .execute(&mut **transaction)
                .await?;
        }
    }

    for table in ["identity_endpoint_models", "identity_endpoint_model_routes"] {
        if table_exists(transaction, table).await?
            && column_exists(transaction, table, "interface_id").await?
            && !column_exists(transaction, table, "endpoint_id").await?
        {
            sqlx::query(&format!(
                "ALTER TABLE {table} RENAME COLUMN interface_id TO endpoint_id"
            ))
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

async fn table_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    name: &str,
) -> Result<bool, StorageError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
    )
    .bind(name)
    .fetch_one(&mut **transaction)
    .await?
        != 0)
}

async fn column_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    table: &str,
    name: &str,
) -> Result<bool, StorageError> {
    #[derive(sqlx::FromRow)]
    struct Column {
        name: String,
    }

    let columns = sqlx::query_as::<_, Column>(&format!("PRAGMA table_info({table})"))
        .fetch_all(&mut **transaction)
        .await?;
    Ok(columns.iter().any(|column| column.name == name))
}

async fn create_identity_schema(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StorageError> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS identities (\
            id TEXT PRIMARY KEY,\
            machine_id TEXT NOT NULL,\
            account_sid TEXT NOT NULL,\
            credential_hash TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            last_active_at TEXT NOT NULL,\
            UNIQUE(machine_id, account_sid)\
        )",
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
        "CREATE TABLE IF NOT EXISTS identity_provider_models (\
            id TEXT PRIMARY KEY,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            model_name TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            UNIQUE(provider_id, model_name)\
        )",
        "CREATE TABLE IF NOT EXISTS identity_endpoint_configs (\
            id TEXT PRIMARY KEY,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            name TEXT NOT NULL,\
            protocol TEXT NOT NULL,\
            token TEXT NOT NULL UNIQUE,\
            created_at TEXT NOT NULL\
        )",
        "CREATE TABLE IF NOT EXISTS identity_endpoint_models (\
            id TEXT PRIMARY KEY,\
            endpoint_id TEXT NOT NULL REFERENCES identity_endpoint_configs(id),\
            model_name TEXT NOT NULL,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            upstream_model TEXT NOT NULL,\
            candidate_order INTEGER NOT NULL DEFAULT 0,\
            created_at TEXT NOT NULL,\
            UNIQUE(endpoint_id, model_name, provider_id, upstream_model)\
        )",
        "CREATE TABLE IF NOT EXISTS identity_endpoint_model_routes (\
            endpoint_id TEXT NOT NULL REFERENCES identity_endpoint_configs(id),\
            model_name TEXT NOT NULL,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            updated_at TEXT NOT NULL,\
            PRIMARY KEY(endpoint_id, model_name)\
        )",
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
        "CREATE TABLE IF NOT EXISTS identity_request_logs (\
            id TEXT PRIMARY KEY,\
            identity_id TEXT NOT NULL REFERENCES identities(id),\
            created_at TEXT NOT NULL,\
            protocol_in TEXT, protocol_out TEXT, protocol_upstream TEXT,\
            endpoint_name TEXT, provider_id TEXT, provider_name TEXT, model_requested TEXT, model_upstream TEXT,\
            proxy_token_id TEXT, status TEXT NOT NULL, http_status INTEGER, error_code TEXT,\
            error_message TEXT, is_streaming INTEGER, input_tokens INTEGER, output_tokens INTEGER,\
            reasoning_tokens INTEGER, cache_read_tokens INTEGER, cache_write_tokens INTEGER,\
            estimated_cost REAL, currency TEXT, latency_ms INTEGER, upstream_latency_ms INTEGER,\
            first_token_ms INTEGER, tool_call_count INTEGER, upstream_request_id TEXT, metadata_json TEXT\
        )",
        "CREATE INDEX IF NOT EXISTS idx_identity_request_logs_identity_beijing_created_at \
         ON identity_request_logs (identity_id, datetime(created_at, '+8 hours'))",
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
    ] {
        sqlx::query(statement).execute(&mut **transaction).await?;
    }
    Ok(())
}

async fn upgrade_endpoint_model_candidates(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StorageError> {
    #[derive(sqlx::FromRow)]
    struct Column {
        name: String,
    }

    let columns = sqlx::query_as::<_, Column>("PRAGMA table_info(identity_endpoint_models)")
        .fetch_all(&mut **transaction)
        .await?;
    if columns
        .iter()
        .any(|column| column.name == "candidate_order")
    {
        return Ok(());
    }

    sqlx::query(
        "CREATE TABLE identity_endpoint_models_replacement (\
            id TEXT PRIMARY KEY,\
            endpoint_id TEXT NOT NULL REFERENCES identity_endpoint_configs(id),\
            model_name TEXT NOT NULL,\
            provider_id TEXT NOT NULL REFERENCES identity_provider_configs(id),\
            upstream_model TEXT NOT NULL,\
            candidate_order INTEGER NOT NULL DEFAULT 0,\
            created_at TEXT NOT NULL,\
            UNIQUE(endpoint_id, model_name, provider_id, upstream_model)\
        )",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO identity_endpoint_models_replacement (\
            id, endpoint_id, model_name, provider_id, upstream_model, candidate_order, created_at\
        ) SELECT id, endpoint_id, model_name, provider_id, upstream_model, 0, created_at \
          FROM identity_endpoint_models",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DROP TABLE identity_endpoint_models")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "ALTER TABLE identity_endpoint_models_replacement RENAME TO identity_endpoint_models",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn upgrade_request_log_columns(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StorageError> {
    #[derive(sqlx::FromRow)]
    struct Column {
        name: String,
    }

    let columns = sqlx::query_as::<_, Column>("PRAGMA table_info(identity_request_logs)")
        .fetch_all(&mut **transaction)
        .await?;
    if columns.iter().any(|column| column.name == "endpoint_name") {
        return Ok(());
    }

    if columns.iter().any(|column| column.name == "interface_name") {
        sqlx::query(
            "ALTER TABLE identity_request_logs RENAME COLUMN interface_name TO endpoint_name",
        )
        .execute(&mut **transaction)
        .await?;
        return Ok(());
    }

    sqlx::query("ALTER TABLE identity_request_logs ADD COLUMN endpoint_name TEXT")
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn discard_unscoped_legacy_schema(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StorageError> {
    #[derive(sqlx::FromRow)]
    struct Column {
        name: String,
    }

    let table_exists = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'provider_configs')",
    )
    .fetch_one(&mut **transaction)
    .await?;
    if table_exists == 0 {
        return Ok(());
    }

    let columns = sqlx::query_as::<_, Column>("PRAGMA table_info(provider_configs)")
        .fetch_all(&mut **transaction)
        .await?;
    if columns.iter().any(|column| column.name == "identity_id") {
        return Ok(());
    }

    tracing::warn!(
        "discarding incompatible unscoped legacy database; existing providers, endpoints, sessions, logs, and secrets will not be migrated"
    );
    for table in [
        "request_logs",
        "response_sessions",
        "interface_models",
        "interface_configs",
        "endpoint_models",
        "endpoint_configs",
        "provider_models",
        "model_aliases",
        "provider_configs",
    ] {
        sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

async fn upgrade_response_sessions_primary_key(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StorageError> {
    #[derive(sqlx::FromRow)]
    struct Column {
        name: String,
        pk: i64,
    }

    let columns = sqlx::query_as::<_, Column>("PRAGMA table_info(identity_response_sessions)")
        .fetch_all(&mut **transaction)
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
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO identity_response_sessions_replacement (\
            response_id, identity_id, previous_response_id, provider_id, model, \
            input_messages_json, output_items_json, created_at\
        ) SELECT response_id, identity_id, previous_response_id, provider_id, model, \
            input_messages_json, output_items_json, created_at \
        FROM identity_response_sessions",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DROP TABLE identity_response_sessions")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "ALTER TABLE identity_response_sessions_replacement \
         RENAME TO identity_response_sessions",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::initialize;

    #[tokio::test]
    async fn migrates_interface_schema_to_endpoint_schema_without_losing_data() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        for statement in [
            "CREATE TABLE identity_interface_configs (\
                id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, name TEXT NOT NULL, \
                protocol TEXT NOT NULL, token TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL\
            )",
            "CREATE TABLE identity_interface_models (\
                id TEXT PRIMARY KEY, interface_id TEXT NOT NULL, model_name TEXT NOT NULL, \
                provider_id TEXT NOT NULL, upstream_model TEXT NOT NULL, created_at TEXT NOT NULL\
            )",
            "CREATE TABLE identity_interface_model_routes (\
                interface_id TEXT NOT NULL, model_name TEXT NOT NULL, provider_id TEXT NOT NULL, \
                updated_at TEXT NOT NULL, PRIMARY KEY(interface_id, model_name)\
            )",
            "CREATE TABLE identity_request_logs (\
                id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, created_at TEXT NOT NULL, \
                interface_name TEXT\
            )",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        sqlx::query(
            "INSERT INTO identity_interface_configs (id, identity_id, name, protocol, token, created_at) \
             VALUES ('endpoint-a', 'identity-a', 'Main', 'openai', 'token-a', '2026-08-23T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        initialize(&pool).await.unwrap();

        let endpoint_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM identity_endpoint_configs WHERE id = 'endpoint-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let endpoint_model_column = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pragma_table_info('identity_endpoint_models') \
             WHERE name = 'endpoint_id'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let endpoint_log_column = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pragma_table_info('identity_request_logs') \
             WHERE name = 'endpoint_name'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(endpoint_name, "Main");
        assert_eq!(endpoint_model_column, 1);
        assert_eq!(endpoint_log_column, 1);
    }
}
