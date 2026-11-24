use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use prelay_server::{store::schema::initialize, test_support::test_empty_database_connection};

const COUNT_COLUMN: &str = "result_count";

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
        .expect("query columns")
        .expect("column count row");
    row.try_get::<i64>("", COUNT_COLUMN).expect("column count") == 1
}

async fn column_default(db: &DatabaseConnection, table: &str, column: &str) -> Option<String> {
    let sql = format!(
        "SELECT column_default FROM information_schema.columns \
         WHERE table_schema = current_schema() \
         AND table_name = '{table}' \
         AND column_name = '{column}'"
    );
    db.query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query column default")
        .expect("column default row")
        .try_get::<Option<String>>("", "column_default")
        .expect("column default")
}

async fn index_exists(db: &DatabaseConnection, table: &str, index: &str) -> bool {
    let sql = format!(
        "SELECT COUNT(*) AS {COUNT_COLUMN} FROM pg_indexes \
         WHERE schemaname = current_schema() \
         AND tablename = '{table}' \
         AND indexname = '{index}'"
    );
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query indexes")
        .expect("index count row");
    row.try_get::<i64>("", COUNT_COLUMN).expect("index count") == 1
}

async fn provider_fk_exists(db: &DatabaseConnection) -> bool {
    let sql = format!(
        "SELECT COUNT(*) AS {COUNT_COLUMN}
         FROM pg_constraint con
         JOIN pg_class rel ON rel.oid = con.conrelid
         JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
         WHERE nsp.nspname = current_schema()
           AND rel.relname = 'identity_response_sessions'
           AND con.contype = 'f'
           AND pg_get_constraintdef(con.oid) LIKE 'FOREIGN KEY (provider_id)%'"
    );
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query constraints")
        .expect("constraint count row");
    row.try_get::<i64>("", COUNT_COLUMN)
        .expect("constraint count")
        > 0
}

async fn assert_exact_response_state_schema(db: &DatabaseConnection) {
    for column in [
        "status",
        "protocol_format",
        "upstream_response_id",
        "request_items_json",
        "response_items_json",
        "expires_at",
    ] {
        assert!(
            column_exists(db, "identity_response_sessions", column).await,
            "missing column {column}"
        );
    }
    for column in ["input_messages_json", "output_items_json"] {
        assert!(
            !column_exists(db, "identity_response_sessions", column).await,
            "identity_response_sessions.{column} must not exist"
        );
    }
    for (column, expected) in [
        ("status", "completed"),
        ("protocol_format", "responses_items_v1"),
    ] {
        let default = column_default(db, "identity_response_sessions", column)
            .await
            .unwrap_or_default();
        assert!(
            default.contains(expected),
            "identity_response_sessions.{column} default must be {expected}, got {default}"
        );
        assert!(
            !default.contains("legacy"),
            "identity_response_sessions.{column} default must not be legacy"
        );
    }
    assert!(
        index_exists(
            db,
            "identity_response_sessions",
            "idx_identity_response_sessions_expires_at"
        )
        .await,
        "missing response state TTL index"
    );
    assert!(
        !provider_fk_exists(db).await,
        "provider foreign key must not block response state history"
    );
}

#[tokio::test]
async fn creates_exact_response_state_schema_for_an_empty_database() {
    let db = test_empty_database_connection().await;
    initialize(&db).await.expect("initialize current schema");

    assert_exact_response_state_schema(&db).await;
}

#[tokio::test]
async fn upgrades_legacy_response_state_schema_and_removes_provider_fk() {
    let db = test_empty_database_connection().await;
    initialize(&db).await.expect("initialize current schema");

    db.execute_unprepared(
        "ALTER TABLE identity_response_sessions
         DROP COLUMN request_items_json,
         DROP COLUMN response_items_json,
         DROP COLUMN upstream_response_id,
         DROP COLUMN status,
         DROP COLUMN protocol_format,
         DROP COLUMN expires_at",
    )
    .await
    .expect("simulate previous schema");
    db.execute_unprepared(
        "ALTER TABLE identity_response_sessions
         ADD CONSTRAINT identity_response_sessions_provider_id_fkey
         FOREIGN KEY (provider_id) REFERENCES identity_provider_configs(id)",
    )
    .await
    .expect("restore legacy provider foreign key");

    initialize(&db).await.expect("upgrade legacy schema");

    assert_exact_response_state_schema(&db).await;
}

async fn count(db: &DatabaseConnection, sql: String) -> i64 {
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
        .await
        .expect("query count")
        .expect("count row");
    row.try_get::<i64>("", COUNT_COLUMN).expect("count column")
}

async fn seed_identity(db: &DatabaseConnection, id: &str) {
    db.execute_unprepared(&format!(
        "INSERT INTO identities
         (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at)
         VALUES ('{id}', 'machine-{id}', 'S-1-5-21-{id}', 'hash', '',
         '2026-11-22T00:00:00Z', '2026-11-22T00:00:00Z')"
    ))
    .await
    .expect("seed identity");
}

async fn seed_response_session(
    db: &DatabaseConnection,
    identity_id: &str,
    response_id: &str,
    protocol_format: &str,
    status: &str,
) {
    db.execute_unprepared(&format!(
        "INSERT INTO identity_response_sessions
         (response_id, identity_id, provider_id, model, status, protocol_format, created_at)
         VALUES ('{response_id}', '{identity_id}', 'provider-1', 'model-1', '{status}',
         '{protocol_format}', '2026-11-22T00:00:00Z')"
    ))
    .await
    .expect("seed response session");
}

async fn session_count(db: &DatabaseConnection, response_id: &str) -> i64 {
    count(
        db,
        format!(
            "SELECT COUNT(*) AS {COUNT_COLUMN} FROM identity_response_sessions \
             WHERE response_id = '{response_id}'"
        ),
    )
    .await
}

async fn cleanup_migration_count(db: &DatabaseConnection) -> i64 {
    count(
        db,
        format!(
            "SELECT COUNT(*) AS {COUNT_COLUMN} FROM prelay_schema_migrations \
             WHERE version = 'responses_legacy_cleanup_v1'"
        ),
    )
    .await
}

#[tokio::test]
async fn removes_responses_legacy_rows_once() {
    let db = test_empty_database_connection().await;
    initialize(&db).await.expect("initialize current schema");
    db.execute_unprepared(
        "DELETE FROM prelay_schema_migrations \
         WHERE version = 'responses_legacy_cleanup_v1'",
    )
    .await
    .expect("simulate an unapplied cleanup migration");
    seed_identity(&db, "identity-legacy").await;
    seed_response_session(
        &db,
        "identity-legacy",
        "resp_legacy",
        "legacy_internal_v1",
        "legacy",
    )
    .await;
    seed_response_session(
        &db,
        "identity-legacy",
        "resp_current",
        "responses_items_v1",
        "completed",
    )
    .await;

    initialize(&db).await.expect("apply legacy cleanup");

    assert_eq!(session_count(&db, "resp_legacy").await, 0);
    assert_eq!(session_count(&db, "resp_current").await, 1);
    assert_eq!(cleanup_migration_count(&db).await, 1);

    initialize(&db).await.expect("rerun legacy cleanup");

    assert_eq!(cleanup_migration_count(&db).await, 1);
}
