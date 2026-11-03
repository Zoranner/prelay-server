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
