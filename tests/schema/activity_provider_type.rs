use prelay_server::{
    schema::{initialize, initialize_with_catalog},
    test_support::{fixture_catalog, test_empty_database_connection},
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use super::initialization::column_exists;

#[tokio::test]
async fn backfills_activity_provider_types_from_existing_provider_records() {
    let db = test_empty_database_connection().await;
    initialize(&db)
        .await
        .expect("initialize the current schema");
    db.execute_unprepared("ALTER TABLE identity_activities DROP COLUMN provider_type")
        .await
        .expect("remove the provider type column");
    db.execute_unprepared(
        "INSERT INTO identities
         (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at)
         VALUES ('activity-identity', 'activity-machine', 'S-1-5-21-activity', 'hash', '',
         '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z')",
    )
    .await
    .expect("seed activity identity");
    db.execute_unprepared(
        "INSERT INTO identity_provider_configs
         (id, identity_id, name, provider_type, visibility, base_url,
          api_key_ciphertext, capabilities_json, models_json, created_at)
         VALUES ('activity-provider', 'activity-identity', '我的 DeepSeek',
         'deepseek', 'private', 'https://provider.example',
         'ciphertext', NULL, NULL, '2026-10-01T00:00:00Z')",
    )
    .await
    .expect("seed activity provider");
    db.execute_unprepared(
        "INSERT INTO identity_activities
         (id, identity_id, created_at, provider_id, provider_name, model_requested, status)
         VALUES
         ('activity-linked', 'activity-identity', '2026-10-01T00:00:00Z',
          'activity-provider', '我的 DeepSeek', 'deepseek-v4-flash', 'success'),
         ('activity-missing-provider', 'activity-identity', '2026-10-01T00:00:00Z',
          'removed-provider', '已删除实例', 'removed-model', 'success')",
    )
    .await
    .expect("seed legacy activities");

    initialize_with_catalog(&db, &fixture_catalog())
        .await
        .expect("migrate and backfill activity provider types");

    assert!(column_exists(&db, "identity_activities", "provider_type").await);
    let rows = db
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id, provider_type FROM identity_activities ORDER BY id".to_owned(),
        ))
        .await
        .expect("query backfilled activities");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0]
            .try_get::<Option<String>>("", "provider_type")
            .expect("linked provider type")
            .as_deref(),
        Some("deepseek")
    );
    assert_eq!(
        rows[1]
            .try_get::<Option<String>>("", "provider_type")
            .expect("missing provider type")
            .as_deref(),
        None
    );
}
