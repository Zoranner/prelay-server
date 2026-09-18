use prelay_server::{schema::initialize, test_support::test_database_connection};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

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

async fn count_rows(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DbBackend::Postgres, sql.to_owned()))
        .await
        .expect("inspect schema")
        .expect("schema count result")
        .try_get::<i64>("", "result_count")
        .unwrap()
}

#[tokio::test]
async fn upgrades_legacy_provider_sharing_schema_without_losing_provider_data() {
    let db = test_database_connection().await;
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
            DbBackend::Postgres,
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

    assert_eq!(
        count_rows(
            &db,
            "SELECT COUNT(*) AS result_count FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = 'identity_provider_shares'",
        )
        .await,
        1
    );
    assert_eq!(
        count_rows(
            &db,
            "SELECT COUNT(*) AS result_count FROM pg_indexes \
             WHERE schemaname = current_schema() \
             AND indexname = 'uq_identity_provider_shares_provider_grantee'",
        )
        .await,
        1
    );
}

#[tokio::test]
async fn rolls_back_legacy_provider_sharing_upgrade_when_index_creation_fails() {
    let db = test_database_connection().await;
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

    assert_eq!(
        count_rows(
            &db,
            "SELECT COUNT(*) AS result_count FROM information_schema.columns \
             WHERE table_schema = current_schema() \
             AND table_name = 'identity_provider_configs' \
             AND column_name = 'visibility'",
        )
        .await,
        0
    );
    assert_eq!(
        count_rows(
            &db,
            "SELECT COUNT(*) AS result_count FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = 'identity_provider_shares'",
        )
        .await,
        0
    );
}
