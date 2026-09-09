use prelay_server::schema::initialize;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};

async fn create_legacy_provider_sharing_tables(db: &DatabaseConnection) {
    for table in [
        "identities",
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
        "CREATE TABLE identity_provider_configs (
            id TEXT PRIMARY KEY,
            identity_id TEXT NOT NULL,
            name TEXT NOT NULL,
            provider_type TEXT NOT NULL,
            base_url TEXT NOT NULL,
            api_key_ciphertext TEXT NOT NULL,
            capabilities_json TEXT,
            created_at TEXT NOT NULL
        )",
    )
    .await
    .expect("create legacy provider config table");
    db.execute_unprepared(
        "CREATE TABLE identity_activities (
            id TEXT PRIMARY KEY,
            identity_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
    )
    .await
    .expect("create current activity table");
}

#[tokio::test]
async fn upgrades_legacy_provider_sharing_schema_without_losing_provider_data() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    create_legacy_provider_sharing_tables(&db).await;
    db.execute_unprepared(
        "INSERT INTO identity_provider_configs
            (id, identity_id, name, provider_type, base_url, api_key_ciphertext,
             capabilities_json, created_at)
         VALUES
            ('provider-legacy', 'identity-legacy', 'legacy-provider', 'openai',
             'https://example.invalid', 'ciphertext', NULL, '2026-09-09T00:00:00Z')",
    )
    .await
    .expect("seed legacy provider");

    initialize(&db)
        .await
        .expect("upgrade the legacy provider sharing schema");

    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, name, visibility
             FROM identity_provider_configs
             WHERE id = 'provider-legacy'"
                .to_owned(),
        ))
        .await
        .expect("query migrated provider")
        .expect("migrated provider remains available");
    assert_eq!(row.try_get::<String>("", "id").unwrap(), "provider-legacy");
    assert_eq!(
        row.try_get::<String>("", "name").unwrap(),
        "legacy-provider"
    );
    assert_eq!(row.try_get::<String>("", "visibility").unwrap(), "private");

    let shares_table = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS result_count
             FROM sqlite_master
             WHERE type = 'table' AND name = 'identity_provider_shares'"
                .to_owned(),
        ))
        .await
        .expect("inspect migrated provider shares table")
        .expect("provider shares table count");
    assert_eq!(shares_table.try_get::<i64>("", "result_count").unwrap(), 1);

    let shares_index = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS result_count
             FROM sqlite_master
             WHERE type = 'index'
               AND name = 'uq_identity_provider_shares_provider_grantee'"
                .to_owned(),
        ))
        .await
        .expect("inspect migrated provider shares index")
        .expect("provider shares index count");
    assert_eq!(shares_index.try_get::<i64>("", "result_count").unwrap(), 1);
}

#[tokio::test]
async fn rolls_back_legacy_provider_sharing_upgrade_when_index_creation_fails() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    create_legacy_provider_sharing_tables(&db).await;
    db.execute_unprepared(
        "CREATE UNIQUE INDEX uq_identity_provider_shares_provider_grantee
         ON identities (id)",
    )
    .await
    .expect("create conflicting index name");

    let error = initialize(&db)
        .await
        .expect_err("conflicting migration index must fail");
    assert!(
        error.to_string().contains("already exists"),
        "unexpected migration error: {error}"
    );

    let visibility_column = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS result_count
             FROM pragma_table_info('identity_provider_configs')
             WHERE name = 'visibility'"
                .to_owned(),
        ))
        .await
        .expect("inspect provider visibility column")
        .expect("provider visibility column count");
    assert_eq!(
        visibility_column
            .try_get::<i64>("", "result_count")
            .unwrap(),
        0
    );

    let shares_table = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS result_count
             FROM sqlite_master
             WHERE type = 'table' AND name = 'identity_provider_shares'"
                .to_owned(),
        ))
        .await
        .expect("inspect rolled back provider shares table")
        .expect("provider shares table count");
    assert_eq!(shares_table.try_get::<i64>("", "result_count").unwrap(), 0);
}
