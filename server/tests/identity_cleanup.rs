use chrono::{Duration, Utc};
use provider_relay_protocol::{CreateInterfaceRequest, CreateProviderRequest, InterfaceModelInput};
use provider_relay_server::storage::{MasterKey, Storage};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

async fn test_storage() -> (Storage, SqlitePool) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create sqlite pool");
    let storage = Storage::initialize(pool.clone(), MasterKey::from_bytes([0; 32]))
        .await
        .expect("initialize storage");
    (storage, pool)
}

#[tokio::test]
async fn cleanup_removes_inactive_identity_and_all_owned_data() {
    let (storage, pool) = test_storage().await;
    let identity = storage
        .register_identity("machine-a", "S-1-5-21-100")
        .await
        .expect("register identity");
    let provider_id = storage
        .create_provider(
            &identity.identity_id,
            CreateProviderRequest {
                name: "Provider".to_string(),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider.example".to_string(),
                api_key: "sk-secret".to_string(),
                capabilities: None,
                models: vec!["upstream-model".to_string()],
            },
        )
        .await
        .expect("create provider");
    let interface = storage
        .create_interface(
            &identity.identity_id,
            CreateInterfaceRequest {
                name: "Interface".to_string(),
                protocol: None,
                models: vec![InterfaceModelInput {
                    model_name: Some("model".to_string()),
                    provider_id: provider_id.clone(),
                    upstream_model: "upstream-model".to_string(),
                }],
            },
        )
        .await
        .expect("create interface");

    sqlx::query(
        "INSERT INTO identity_model_aliases (id, identity_id, alias, provider_id, upstream_model, \
         downstream_protocols_json, enabled, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("alias")
    .bind(&identity.identity_id)
    .bind("alias-model")
    .bind(&provider_id)
    .bind("upstream-model")
    .bind("[\"responses\"]")
    .bind(1)
    .bind(Utc::now().to_rfc3339())
    .execute(&pool)
    .await
    .expect("create alias");
    sqlx::query(
        "INSERT INTO identity_response_sessions (response_id, identity_id, previous_response_id, provider_id, \
         model, input_messages_json, output_items_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("response")
    .bind(&identity.identity_id)
    .bind(Option::<String>::None)
    .bind(&provider_id)
    .bind("model")
    .bind("[]")
    .bind("[]")
    .bind(Utc::now().to_rfc3339())
    .execute(&pool)
    .await
    .expect("create session");
    sqlx::query(
        "INSERT INTO identity_request_logs (id, identity_id, created_at, status) VALUES (?, ?, ?, ?)",
    )
    .bind("log")
    .bind(&identity.identity_id)
    .bind(Utc::now().to_rfc3339())
    .bind("success")
    .execute(&pool)
    .await
    .expect("create log");
    sqlx::query("UPDATE identities SET last_active_at = ? WHERE id = ?")
        .bind("2026-01-01T00:00:00+00:00")
        .bind(&identity.identity_id)
        .execute(&pool)
        .await
        .expect("make identity inactive");

    assert_eq!(
        storage
            .delete_inactive_identities(Utc::now(), Duration::days(90))
            .await
            .expect("clean inactive identities"),
        1
    );
    for table in [
        "identities",
        "identity_provider_configs",
        "identity_provider_models",
        "identity_interface_configs",
        "identity_interface_models",
        "identity_response_sessions",
        "identity_request_logs",
        "identity_model_aliases",
    ] {
        let count = sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .expect("count owned resources");
        assert_eq!(count, 0, "{table} still has an inactive identity resource");
    }
    assert_eq!(interface.models.len(), 1);
}

#[tokio::test]
async fn initialization_discards_unscoped_legacy_database() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("create sqlite pool");
    sqlx::query("CREATE TABLE provider_configs (id TEXT PRIMARY KEY, api_key TEXT NOT NULL)")
        .execute(&pool)
        .await
        .expect("create legacy provider table");
    sqlx::query("CREATE TABLE provider_models (id TEXT PRIMARY KEY, provider_id TEXT NOT NULL)")
        .execute(&pool)
        .await
        .expect("create legacy provider model table");
    sqlx::query("INSERT INTO provider_configs (id, api_key) VALUES (?, ?)")
        .bind("legacy-provider")
        .bind("sk-legacy")
        .execute(&pool)
        .await
        .expect("store legacy secret");

    let _storage = Storage::initialize(pool.clone(), MasterKey::from_bytes([0; 32]))
        .await
        .expect("replace legacy schema");

    let legacy_table_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('provider_configs', 'provider_models')",
    )
    .fetch_one(&pool)
    .await
    .expect("inspect legacy tables");
    assert_eq!(legacy_table_count, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM identities")
            .fetch_one(&pool)
            .await
            .expect("count identities"),
        0
    );
}
