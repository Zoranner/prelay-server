use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use prelay_server::{schema::initialize, test_support::test_empty_database_connection};

const TABLES: [&str; 11] = [
    "identities",
    "identity_provider_configs",
    "identity_provider_shares",
    "identity_endpoint_configs",
    "identity_endpoint_models",
    "identity_endpoint_model_routes",
    "identity_response_sessions",
    "identity_activities",
    "activity_contents",
    "memories",
    "memory_sources",
];
const COUNT_COLUMN: &str = "result_count";

async fn table_exists(db: &DatabaseConnection, table: &str) -> bool {
    let sql = format!(
        "SELECT COUNT(*) AS {COUNT_COLUMN} FROM information_schema.tables \
         WHERE table_schema = current_schema() AND table_name = '{table}'"
    );
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap();
    row.try_get::<i64>("", COUNT_COLUMN).unwrap() == 1
}

async fn column_type(db: &DatabaseConnection, table: &str, column: &str) -> String {
    let sql = format!(
        "SELECT data_type FROM information_schema.columns \
         WHERE table_schema = current_schema() \
         AND table_name = '{table}' \
         AND column_name = '{column}'"
    );
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap();
    row.try_get("", "data_type").unwrap()
}

async fn column_exists(db: &DatabaseConnection, table: &str, column: &str) -> bool {
    let sql = format!(
        "SELECT COUNT(*) AS {COUNT_COLUMN} FROM information_schema.columns \
         WHERE table_schema = current_schema() \
         AND table_name = '{table}' \
         AND column_name = '{column}'"
    );
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap();
    row.try_get::<i64>("", COUNT_COLUMN).unwrap() == 1
}

async fn index_exists(db: &DatabaseConnection, table: &str, index: &str) -> bool {
    let sql = format!(
        "SELECT COUNT(*) AS {COUNT_COLUMN} FROM pg_indexes \
         WHERE schemaname = current_schema() AND tablename = '{table}' AND indexname = '{index}'"
    );
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap();
    row.try_get::<i64>("", COUNT_COLUMN).unwrap() == 1
}

async fn assert_string_column(db: &DatabaseConnection, table: &str, column: &str) {
    let column_type = column_type(db, table, column).await.to_ascii_uppercase();
    assert!(
        matches!(
            column_type.as_str(),
            "TEXT" | "VARCHAR" | "CHARACTER VARYING"
        ),
        "{table}.{column} must be stored as text, got {column_type}"
    );
}

async fn assert_complete_schema(db: &DatabaseConnection) {
    for table in TABLES {
        assert!(table_exists(db, table).await, "missing table: {table}");
    }
    assert_string_column(db, "identity_provider_configs", "visibility").await;
    for column in ["provider_id", "grantee_identity_id", "created_at"] {
        assert_string_column(db, "identity_provider_shares", column).await;
    }
    for column in ["api_key", "api_key_ciphertext", "credential", "token"] {
        assert!(
            !column_exists(db, "identity_provider_shares", column).await,
            "identity_provider_shares must not store credentials in {column}"
        );
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

    db.execute_unprepared(
        "INSERT INTO identity_provider_configs
            (id, identity_id, name, provider_type, base_url, api_key_ciphertext, created_at)
         VALUES
            ('provider-1', 'identity-1', 'Provider', 'openai', 'https://example.test',
             'ciphertext', '2026-08-23T00:00:00Z')",
    )
    .await
    .unwrap();
    db.execute_unprepared(
        "INSERT INTO identities
            (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at)
         VALUES
            ('identity-2', 'machine-2', 'S-1-5-22', 'hash', '', '2026-08-23T00:00:00Z',
             '2026-08-23T00:00:00Z')",
    )
    .await
    .unwrap();
    db.execute_unprepared(
        "INSERT INTO identity_provider_shares
            (provider_id, grantee_identity_id, created_at)
         VALUES
            ('provider-1', 'identity-2', '2026-08-23T00:00:00Z')",
    )
    .await
    .unwrap();
    let duplicate_share = db
        .execute_unprepared(
            "INSERT INTO identity_provider_shares
                (provider_id, grantee_identity_id, created_at)
             VALUES
                ('provider-1', 'identity-2', '2026-08-23T00:00:01Z')",
        )
        .await;
    assert!(duplicate_share.is_err());

    let missing_provider_share = db
        .execute_unprepared(
            "INSERT INTO identity_provider_shares
                (provider_id, grantee_identity_id, created_at)
             VALUES
                ('missing-provider', 'identity-2', '2026-08-23T00:00:00Z')",
        )
        .await;
    assert!(missing_provider_share.is_err());
    let missing_grantee_share = db
        .execute_unprepared(
            "INSERT INTO identity_provider_shares
                (provider_id, grantee_identity_id, created_at)
             VALUES
                ('provider-1', 'missing-identity', '2026-08-23T00:00:00Z')",
        )
        .await;
    assert!(missing_grantee_share.is_err());

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
        assert!(
            column_type(db, "identity_activities", column)
                .await
                .eq_ignore_ascii_case("BIGINT"),
            "{column} must map to an i64-compatible BIGINT"
        );
    }
    for column in ["estimated_cost", "currency"] {
        assert!(
            !column_exists(db, "identity_activities", column).await,
            "identity_activities.{column} must not exist"
        );
    }
    assert_eq!(
        column_type(db, "identity_activities", "is_streaming")
            .await
            .to_ascii_uppercase(),
        "BOOLEAN"
    );
    assert!(
        column_type(db, "identity_endpoint_models", "candidate_order")
            .await
            .eq_ignore_ascii_case("BIGINT"),
        "candidate_order must map to an i64-compatible BIGINT"
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
        "BIGINT"
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
        matches!(confidence_type.as_str(), "DOUBLE PRECISION" | "REAL"),
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
             VALUES ('content-1', 'missing', '', '', 'hash', 'pending', false, 0, '2026-08-31T00:00:00Z', '2026-08-31T00:00:00Z')",
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

    assert!(
        index_exists(
            db,
            "identity_activities",
            "idx_identity_activities_identity_created_at"
        )
        .await
    );
    assert!(
        index_exists(
            db,
            "identity_provider_shares",
            "uq_identity_provider_shares_provider_grantee"
        )
        .await
    );
}

#[tokio::test]
async fn initializes_the_complete_identity_schema_with_core_constraints() {
    let db = test_empty_database_connection().await;

    initialize(&db).await.unwrap();
    assert_complete_schema(&db).await;
}

#[tokio::test]
async fn reuses_the_current_identity_schema_without_changes() {
    let db = test_empty_database_connection().await;

    initialize(&db).await.unwrap();
    initialize(&db).await.unwrap();
    assert_complete_schema(&db).await;
}
