use prelay_server::schema::initialize;
use sea_orm::{ConnectionTrait, Database, DbBackend, EntityTrait, Statement};

#[tokio::test]
async fn initializes_an_empty_database_without_migration_metadata() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");

    initialize(&db)
        .await
        .expect("initialize the current schema");
    prelay_server::entity::identities::Entity::find()
        .all(&db)
        .await
        .expect("identities table exists");

    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'seaql_migrations'"
                .to_owned(),
        ))
        .await
        .expect("inspect SQLite schema")
        .expect("migration table count result");
    assert_eq!(row.try_get::<i64>("", "COUNT(*)").unwrap(), 0);
}

#[tokio::test]
async fn rejects_a_partially_initialized_database() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    db.execute_unprepared("CREATE TABLE identities (id TEXT PRIMARY KEY)")
        .await
        .expect("create an incomplete schema");

    let error = initialize(&db)
        .await
        .expect_err("partial schemas require a new database deployment");
    assert!(error.to_string().contains("incomplete"));
}

#[tokio::test]
async fn migrates_the_complete_legacy_activity_table_without_losing_rows() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    for table in [
        "identities",
        "identity_provider_configs",
        "identity_provider_models",
        "identity_endpoint_configs",
        "identity_endpoint_models",
        "identity_endpoint_model_routes",
        "identity_response_sessions",
        "identity_model_aliases",
    ] {
        db.execute_unprepared(&format!("CREATE TABLE {table} (id TEXT PRIMARY KEY)"))
            .await
            .expect("create legacy companion table");
    }
    db.execute_unprepared(
        "CREATE TABLE identity_activities (id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, created_at TEXT NOT NULL)",
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
            DbBackend::Sqlite,
            "SELECT id FROM identity_activities WHERE id = 'activity-1'".to_owned(),
        ))
        .await
        .expect("query migrated activity")
        .expect("legacy activity remains available");
    assert_eq!(row.try_get::<String>("", "id").unwrap(), "activity-1");
}

#[tokio::test]
async fn removes_legacy_cost_columns_from_an_existing_activity_table() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    for table in [
        "identities",
        "identity_provider_configs",
        "identity_provider_models",
        "identity_endpoint_configs",
        "identity_endpoint_models",
        "identity_endpoint_model_routes",
        "identity_response_sessions",
        "identity_model_aliases",
    ] {
        db.execute_unprepared(&format!("CREATE TABLE {table} (id TEXT PRIMARY KEY)"))
            .await
            .expect("create existing companion table");
    }
    db.execute_unprepared(
        "CREATE TABLE identity_activities (
            id TEXT PRIMARY KEY,
            identity_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            estimated_cost DOUBLE,
            currency TEXT
        )",
    )
    .await
    .expect("create existing activity table with legacy cost columns");

    initialize(&db).await.expect("initialize existing schema");

    for column in ["estimated_cost", "currency"] {
        let row = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS result_count FROM pragma_table_info('identity_activities') \
                     WHERE name = '{column}'"
                ),
            ))
            .await
            .expect("inspect activity columns")
            .expect("activity column count");
        assert_eq!(
            row.try_get::<i64>("", "result_count").unwrap(),
            0,
            "legacy cost column {column} must be removed"
        );
    }
}

#[tokio::test]
async fn migrates_stale_activity_content_once_during_schema_initialization() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");

    initialize(&db).await.expect("initialize current schema");
    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS prelay_schema_migrations (
            version VARCHAR(128) PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .await
    .expect("create migration history");
    db.execute_unprepared(
        "DELETE FROM prelay_schema_migrations
         WHERE version = 'activity_content_lifecycle_v1'",
    )
    .await
    .expect("reset activity content migration");
    db.execute_unprepared(
        "INSERT INTO identities
            (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at)
         VALUES
            ('identity-migration', 'machine-migration', 'sid-migration', 'hash', '',
             '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')",
    )
    .await
    .expect("insert migration identity");
    db.execute_unprepared(
        "INSERT INTO identity_activities (id, identity_id, created_at, status)
         VALUES ('activity-migration', 'identity-migration', '2026-09-01T00:00:00Z', 'success')",
    )
    .await
    .expect("insert migration activity");
    db.execute_unprepared(
        "INSERT INTO activity_contents
            (id, activity_id, input_text, output_text, content_hash, status, is_truncated,
             attempts, created_at, updated_at)
         VALUES
            ('content-migration', 'activity-migration', 'input', 'output', 'hash', 'capturing',
             0, 0, '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')",
    )
    .await
    .expect("insert stale activity content");

    initialize(&db)
        .await
        .expect("apply activity content migration");

    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT status, input_text, output_text
             FROM activity_contents
             WHERE id = 'content-migration'"
                .to_owned(),
        ))
        .await
        .expect("query migrated activity content")
        .expect("migrated activity content exists");
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "pending");
    assert_eq!(row.try_get::<String>("", "input_text").unwrap(), "input");
    assert_eq!(row.try_get::<String>("", "output_text").unwrap(), "output");

    initialize(&db)
        .await
        .expect("re-running schema initialization is idempotent");
    let migration_count = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS migration_count
             FROM prelay_schema_migrations
             WHERE version = 'activity_content_lifecycle_v1'"
                .to_owned(),
        ))
        .await
        .expect("query migration history")
        .expect("migration history exists");
    assert_eq!(
        migration_count
            .try_get::<i64>("", "migration_count")
            .unwrap(),
        1
    );
}
