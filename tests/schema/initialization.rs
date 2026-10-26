use prelay_server::{schema::initialize, test_support::test_empty_database_connection};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, Statement};

pub(super) async fn column_exists(db: &DatabaseConnection, table: &str, column: &str) -> bool {
    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "SELECT COUNT(*) AS result_count FROM information_schema.columns \
                 WHERE table_schema = current_schema() \
                 AND table_name = '{table}' \
                 AND column_name = '{column}'"
            ),
        ))
        .await
        .expect("inspect schema columns")
        .expect("column count result");
    row.try_get::<i64>("", "result_count").unwrap() == 1
}

async fn table_exists(db: &DatabaseConnection, table: &str) -> bool {
    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "SELECT COUNT(*) AS result_count FROM information_schema.tables \
                 WHERE table_schema = current_schema() AND table_name = '{table}'"
            ),
        ))
        .await
        .expect("inspect schema tables")
        .expect("table count result");
    row.try_get::<i64>("", "result_count").unwrap() == 1
}

async fn create_existing_base_tables(db: &DatabaseConnection) {
    for table in [
        "identities",
        "identity_endpoint_configs",
        "identity_endpoint_models",
        "identity_endpoint_model_routes",
    ] {
        db.execute_unprepared(&format!("CREATE TABLE {table} (id TEXT PRIMARY KEY)"))
            .await
            .expect("create existing companion table");
    }
    create_legacy_response_session_table(db).await;
    db.execute_unprepared(
        "CREATE TABLE identity_provider_configs (
            id TEXT PRIMARY KEY,
            visibility TEXT NOT NULL
        )",
    )
    .await
    .expect("create existing provider config table");
    db.execute_unprepared(
        "CREATE TABLE identity_provider_shares (
            provider_id TEXT NOT NULL,
            grantee_identity_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .await
    .expect("create existing provider shares table");
    db.execute_unprepared(
        "CREATE UNIQUE INDEX uq_identity_provider_shares_provider_grantee
         ON identity_provider_shares (provider_id, grantee_identity_id)",
    )
    .await
    .expect("create existing provider shares index");
}

async fn create_legacy_response_session_table(db: &DatabaseConnection) {
    db.execute_unprepared(
        "CREATE TABLE identity_response_sessions (
            response_id TEXT NOT NULL,
            identity_id TEXT NOT NULL,
            previous_response_id TEXT,
            provider_id TEXT NOT NULL,
            model TEXT NOT NULL,
            input_messages_json TEXT NOT NULL,
            output_items_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (response_id, identity_id)
        )",
    )
    .await
    .expect("create legacy response session table");
}

#[tokio::test]
async fn initializes_an_empty_database_without_migration_metadata() {
    let db = test_empty_database_connection().await;

    initialize(&db)
        .await
        .expect("initialize the current schema");
    prelay_server::entity::identities::Entity::find()
        .all(&db)
        .await
        .expect("identities table exists");

    assert!(!table_exists(&db, "seaql_migrations").await);
}

#[tokio::test]
async fn initializes_provider_sharing_structures_in_an_empty_database() {
    let db = test_empty_database_connection().await;

    initialize(&db)
        .await
        .expect("initialize the current schema");

    for (table, column) in [
        ("identity_provider_configs", "visibility"),
        ("identity_provider_shares", "provider_id"),
        ("identity_provider_shares", "grantee_identity_id"),
        ("identity_provider_shares", "created_at"),
    ] {
        assert!(
            column_exists(&db, table, column).await,
            "missing {table}.{column}"
        );
    }
}

#[tokio::test]
async fn rejects_a_partially_initialized_database() {
    let db = test_empty_database_connection().await;
    db.execute_unprepared("CREATE TABLE identities (id TEXT PRIMARY KEY)")
        .await
        .expect("create an incomplete schema");

    let error = initialize(&db)
        .await
        .expect_err("partial schemas require a new database deployment");
    assert!(error.to_string().contains("incomplete"));
}

#[tokio::test]
async fn rejects_complete_table_names_without_provider_visibility_column() {
    let db = test_empty_database_connection().await;
    for table in [
        "identities",
        "identity_provider_configs",
        "identity_provider_shares",
        "identity_endpoint_configs",
        "identity_endpoint_models",
        "identity_endpoint_model_routes",
    ] {
        db.execute_unprepared(&format!("CREATE TABLE {table} (id TEXT PRIMARY KEY)"))
            .await
            .expect("create complete table-name set");
    }
    create_legacy_response_session_table(&db).await;
    db.execute_unprepared(
        "CREATE TABLE identity_activities (
            id TEXT PRIMARY KEY,
            identity_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .await
    .expect("create current activity table");

    let error = initialize(&db)
        .await
        .expect_err("schema without provider visibility must be rejected");
    assert!(error.to_string().contains("database schema is incomplete"));
}

#[tokio::test]
async fn rejects_provider_shares_without_required_unique_index() {
    let db = test_empty_database_connection().await;
    for table in [
        "identities",
        "identity_endpoint_configs",
        "identity_endpoint_models",
        "identity_endpoint_model_routes",
    ] {
        db.execute_unprepared(&format!("CREATE TABLE {table} (id TEXT PRIMARY KEY)"))
            .await
            .expect("create complete table-name set");
    }
    create_legacy_response_session_table(&db).await;
    db.execute_unprepared(
        "CREATE TABLE identity_provider_configs (
            id TEXT PRIMARY KEY,
            visibility TEXT NOT NULL
        )",
    )
    .await
    .expect("create provider config table");
    db.execute_unprepared(
        "CREATE TABLE identity_provider_shares (
            provider_id TEXT NOT NULL,
            grantee_identity_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .await
    .expect("create provider shares table without its index");
    db.execute_unprepared(
        "CREATE TABLE identity_activities (
            id TEXT PRIMARY KEY,
            identity_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .await
    .expect("create current activity table");

    let error = initialize(&db)
        .await
        .expect_err("provider shares without its unique index must be rejected");
    assert!(error.to_string().contains("database schema is incomplete"));
}

#[tokio::test]
async fn migrates_the_complete_legacy_activity_table_without_losing_rows() {
    let db = test_empty_database_connection().await;
    create_existing_base_tables(&db).await;
    db.execute_unprepared(
        "CREATE TABLE identity_activities (
            id TEXT PRIMARY KEY,
            identity_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            status TEXT
        )",
    )
    .await
    .expect("create legacy activity table");
    db.execute_unprepared(
        "INSERT INTO identity_activities (id, identity_id, created_at) \
         VALUES ('activity-1', 'identity-1', '2026-08-31T00:00:00Z')",
    )
    .await
    .expect("seed legacy activity");

    initialize(&db)
        .await
        .expect("migrate complete legacy schema");

    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id FROM identity_activities WHERE id = 'activity-1'".to_owned(),
        ))
        .await
        .expect("query migrated activity")
        .expect("legacy activity remains available");
    assert_eq!(row.try_get::<String>("", "id").unwrap(), "activity-1");
}

#[tokio::test]
async fn removes_legacy_cost_columns_from_an_existing_activity_table() {
    let db = test_empty_database_connection().await;
    create_existing_base_tables(&db).await;
    db.execute_unprepared(
        "CREATE TABLE identity_activities (
            id TEXT PRIMARY KEY,
            identity_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            status TEXT,
            estimated_cost DOUBLE PRECISION,
            currency TEXT
        )",
    )
    .await
    .expect("create existing activity table with legacy cost columns");

    initialize(&db).await.expect("initialize existing schema");

    for column in ["estimated_cost", "currency"] {
        assert!(
            !column_exists(&db, "identity_activities", column).await,
            "legacy cost column {column} must be removed"
        );
    }
}

#[tokio::test]
async fn drops_legacy_activity_contents_and_records_rollout_migration() {
    let db = test_empty_database_connection().await;

    initialize(&db).await.expect("initialize current schema");
    db.execute_unprepared(
        "CREATE TABLE activity_contents (
            id TEXT PRIMARY KEY,
            activity_id TEXT NOT NULL,
            input_text TEXT NOT NULL,
            output_text TEXT NOT NULL
        )",
    )
    .await
    .expect("create legacy activity content table");
    db.execute_unprepared(
        "INSERT INTO prelay_schema_migrations (version)
         VALUES ('activity_content_lifecycle_v1')",
    )
    .await
    .expect("record the legacy migration");

    initialize(&db).await.expect("drop legacy activity content");
    assert!(!table_exists(&db, "activity_contents").await);
    assert!(table_exists(&db, "activity_rollouts").await);

    initialize(&db)
        .await
        .expect("re-running schema initialization is idempotent");
    let migration_count = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT COUNT(*) AS migration_count
             FROM prelay_schema_migrations
             WHERE version IN ('activity_content_lifecycle_v1', 'activity_rollout_v1')"
                .to_owned(),
        ))
        .await
        .expect("query migration history")
        .expect("migration history exists");
    assert_eq!(
        migration_count
            .try_get::<i64>("", "migration_count")
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn adds_models_column_when_migrating_an_existing_provider_table() {
    let db = test_empty_database_connection().await;
    initialize(&db)
        .await
        .expect("initialize the current schema");
    db.execute_unprepared("ALTER TABLE identity_provider_configs DROP COLUMN models_json")
        .await
        .expect("drop the models column");

    initialize(&db).await.expect("migrate the existing schema");

    assert!(column_exists(&db, "identity_provider_configs", "models_json").await);
}

#[tokio::test]
async fn backfills_conversation_id_for_existing_response_sessions() {
    let db = test_empty_database_connection().await;
    initialize(&db)
        .await
        .expect("initialize the current schema");
    db.execute_unprepared("ALTER TABLE identity_response_sessions DROP COLUMN conversation_id")
        .await
        .expect("remove the conversation id column");
    db.execute_unprepared(
        "INSERT INTO identities
         (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at)
         VALUES ('legacy-identity', 'legacy-machine', 'S-1-5-21-legacy', 'hash', '',
         '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z')",
    )
    .await
    .expect("seed legacy identity");
    db.execute_unprepared(
        "INSERT INTO identity_provider_configs
         (id, identity_id, name, provider_type, visibility, base_url,
          api_key_ciphertext, capabilities_json, models_json, created_at)
         VALUES ('legacy-provider', 'legacy-identity', 'Legacy Provider',
         'openai_compatible', 'private', 'https://provider.example',
         'ciphertext', NULL, NULL, '2026-10-01T00:00:00Z')",
    )
    .await
    .expect("seed legacy provider");
    db.execute_unprepared(
        "INSERT INTO identity_response_sessions
         (response_id, identity_id, previous_response_id, provider_id, model,
          input_messages_json, output_items_json, created_at)
         VALUES ('resp_legacy', 'legacy-identity', NULL, 'legacy-provider',
         'legacy-model', '[]', '[]', '2026-10-01T00:00:00Z')",
    )
    .await
    .expect("seed legacy response session");

    initialize(&db)
        .await
        .expect("backfill response session conversation ids");

    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT conversation_id FROM identity_response_sessions
             WHERE response_id = 'resp_legacy' AND identity_id = 'legacy-identity'"
                .to_owned(),
        ))
        .await
        .expect("query backfilled response session")
        .expect("response session remains available");
    assert_eq!(
        row.try_get::<Option<String>>("", "conversation_id")
            .expect("conversation id column")
            .as_deref(),
        Some("resp_legacy")
    );
}
