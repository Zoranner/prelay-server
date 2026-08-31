use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};

use prelay_server::schema::initialize;

const TABLES: [&str; 12] = [
    "identities",
    "identity_provider_configs",
    "identity_provider_models",
    "identity_endpoint_configs",
    "identity_endpoint_models",
    "identity_endpoint_model_routes",
    "identity_response_sessions",
    "identity_activities",
    "identity_model_aliases",
    "activity_contents",
    "memories",
    "memory_sources",
];
const COUNT_COLUMN: &str = "result_count";

async fn table_exists(db: &DatabaseConnection, table: &str) -> bool {
    let sql = match db.get_database_backend() {
        DbBackend::Sqlite => {
            format!(
                "SELECT COUNT(*) AS {COUNT_COLUMN} FROM sqlite_master \
                 WHERE type = 'table' AND name = '{table}'"
            )
        }
        DbBackend::Postgres => format!(
            "SELECT COUNT(*) AS {COUNT_COLUMN} FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = '{table}'"
        ),
        _ => unreachable!("only SQLite and PostgreSQL are supported"),
    };
    let statement = Statement::from_string(db.get_database_backend(), sql);
    let row = db.query_one_raw(statement).await.unwrap().unwrap();
    row.try_get::<i64>("", COUNT_COLUMN).unwrap() == 1
}

async fn column_type(db: &DatabaseConnection, table: &str, column: &str) -> String {
    let sql = match db.get_database_backend() {
        DbBackend::Sqlite => {
            format!("SELECT type FROM pragma_table_info('{table}') WHERE name = '{column}'")
        }
        DbBackend::Postgres => format!(
            "SELECT data_type FROM information_schema.columns \
             WHERE table_schema = current_schema() \
             AND table_name = '{table}' \
             AND column_name = '{column}'"
        ),
        _ => unreachable!("only SQLite and PostgreSQL are supported"),
    };
    let statement = Statement::from_string(db.get_database_backend(), sql);
    let row = db.query_one_raw(statement).await.unwrap().unwrap();
    let column_name = if db.get_database_backend() == DbBackend::Sqlite {
        "type"
    } else {
        "data_type"
    };
    row.try_get("", column_name).unwrap()
}

async fn assert_string_column(db: &DatabaseConnection, table: &str, column: &str) {
    let column_type = column_type(db, table, column).await.to_ascii_uppercase();
    assert!(
        matches!(column_type.as_str(), "TEXT" | "VARCHAR"),
        "{table}.{column} must be stored as text, got {column_type}"
    );
}

async fn assert_complete_schema(db: &DatabaseConnection) {
    for table in TABLES {
        assert!(table_exists(db, table).await, "missing table: {table}");
    }

    db.execute_unprepared(
        "INSERT INTO identities (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at) \
         VALUES ('identity-1', 'machine-1', 'S-1-5-21', 'hash', '', '2026-08-23T00:00:00Z', '2026-08-23T00:00:00Z')",
    )
    .await
    .unwrap();

    let duplicate_identity = db
        .execute_unprepared(
            "INSERT INTO identities (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at) \
             VALUES ('identity-2', 'machine-1', 'S-1-5-21', 'hash', '', '2026-08-23T00:00:00Z', '2026-08-23T00:00:00Z')",
        )
        .await;
    assert!(duplicate_identity.is_err());

    let missing_identity = db
        .execute_unprepared(
            "INSERT INTO identity_provider_configs (id, identity_id, name, provider_type, base_url, api_key_ciphertext, created_at) \
             VALUES ('provider-1', 'missing', 'Provider', 'openai', 'https://example.test', 'ciphertext', '2026-08-23T00:00:00Z')",
        )
        .await;
    assert!(missing_identity.is_err());

    for column in [
        "http_status",
        "input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "latency_ms",
        "upstream_latency_ms",
        "first_token_ms",
        "tool_call_count",
    ] {
        let expected_type = match db.get_database_backend() {
            DbBackend::Sqlite => "INTEGER",
            DbBackend::Postgres => "BIGINT",
            _ => unreachable!("only SQLite and PostgreSQL are supported"),
        };
        assert!(
            column_type(db, "identity_activities", column)
                .await
                .eq_ignore_ascii_case(expected_type),
            "{column} must map to an i64-compatible {expected_type}"
        );
    }
    let expected_integer_type = match db.get_database_backend() {
        DbBackend::Sqlite => "INTEGER",
        DbBackend::Postgres => "BIGINT",
        _ => unreachable!("only SQLite and PostgreSQL are supported"),
    };
    assert_eq!(
        column_type(db, "identity_activities", "is_streaming")
            .await
            .to_ascii_uppercase(),
        "BOOLEAN"
    );
    assert!(
        column_type(db, "identity_endpoint_models", "candidate_order")
            .await
            .eq_ignore_ascii_case(expected_integer_type),
        "candidate_order must map to an i64-compatible {expected_integer_type}"
    );
    assert_eq!(
        column_type(db, "identity_model_aliases", "enabled")
            .await
            .to_ascii_uppercase(),
        "BOOLEAN"
    );

    for column in [
        "activity_id",
        "input_text",
        "output_text",
        "content_hash",
        "status",
        "next_attempt_at",
        "lease_owner",
        "lease_expires_at",
        "last_error",
        "completed_at",
    ] {
        assert_string_column(db, "activity_contents", column).await;
    }
    assert_eq!(
        column_type(db, "activity_contents", "is_truncated")
            .await
            .to_ascii_uppercase(),
        "BOOLEAN"
    );
    assert_eq!(
        column_type(db, "activity_contents", "attempts")
            .await
            .to_ascii_uppercase(),
        "INTEGER"
    );
    for column in [
        "normalized_key",
        "conflict_key",
        "kind",
        "status",
        "content",
        "created_at",
        "updated_at",
    ] {
        assert_string_column(db, "memories", column).await;
    }
    let confidence_type = column_type(db, "memories", "confidence")
        .await
        .to_ascii_uppercase();
    assert!(
        matches!(confidence_type.as_str(), "DOUBLE" | "REAL"),
        "memories.confidence must be stored as a floating-point value, got {confidence_type}"
    );
    for column in [
        "memory_id",
        "identity_id",
        "evidence",
        "evidence_hash",
        "observed_at",
    ] {
        assert_string_column(db, "memory_sources", column).await;
    }

    let activity_content_without_activity = db
        .execute_unprepared(
            "INSERT INTO activity_contents (id, activity_id, input_text, output_text, content_hash, status, is_truncated, attempts, created_at, updated_at) \
             VALUES ('content-1', 'missing', '', '', 'hash', 'pending', 0, 0, '2026-08-31T00:00:00Z', '2026-08-31T00:00:00Z')",
        )
        .await;
    assert!(activity_content_without_activity.is_err());

    db.execute_unprepared(
        "INSERT INTO memories (id, normalized_key, kind, status, content, confidence, created_at, updated_at) \
         VALUES ('memory-1', 'preference:example', 'preference', 'active', 'example', 0.9, '2026-08-31T00:00:00Z', '2026-08-31T00:00:00Z')",
    )
    .await
    .unwrap();
    db.execute_unprepared(
        "INSERT INTO memory_sources (id, memory_id, identity_id, evidence, evidence_hash, observed_at, created_at) \
         VALUES ('source-1', 'memory-1', 'deleted-identity', 'evidence', 'evidence-hash', '2026-08-31T00:00:00Z', '2026-08-31T00:00:00Z')",
    )
    .await
    .unwrap();
    let duplicate_memory = db
        .execute_unprepared(
            "INSERT INTO memories (id, normalized_key, kind, status, content, confidence, created_at, updated_at) \
             VALUES ('memory-2', 'preference:example', 'preference', 'active', 'other', 0.8, '2026-08-31T00:00:00Z', '2026-08-31T00:00:00Z')",
        )
        .await;
    assert!(duplicate_memory.is_err());

    let index_sql = match db.get_database_backend() {
        DbBackend::Sqlite => {
            "SELECT COUNT(*) AS result_count FROM pragma_index_list('identity_activities') \
             WHERE name = 'idx_identity_activities_identity_created_at'"
                .to_owned()
        }
        DbBackend::Postgres => "SELECT COUNT(*) AS result_count FROM pg_indexes \
             WHERE schemaname = current_schema() \
             AND tablename = 'identity_activities' \
             AND indexname = 'idx_identity_activities_identity_created_at'"
            .to_owned(),
        _ => unreachable!("only SQLite and PostgreSQL are supported"),
    };
    let index_statement = Statement::from_string(db.get_database_backend(), index_sql);
    let row = db.query_one_raw(index_statement).await.unwrap().unwrap();
    assert_eq!(row.try_get::<i64>("", COUNT_COLUMN).unwrap(), 1);
}

#[tokio::test]
async fn initializes_the_complete_identity_schema_with_core_constraints() {
    let db = Database::connect("sqlite::memory:").await.unwrap();

    initialize(&db).await.unwrap();
    assert_complete_schema(&db).await;
}

#[tokio::test]
async fn reuses_the_current_identity_schema_without_changes() {
    let db = Database::connect("sqlite::memory:").await.unwrap();

    initialize(&db).await.unwrap();
    initialize(&db).await.unwrap();
    assert_complete_schema(&db).await;
}
