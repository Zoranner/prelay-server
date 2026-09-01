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
