use chrono::{Duration, TimeZone, Utc};
use prelay_protocol::{CreateInterfaceRequest, CreateProviderRequest, InterfaceModelInput};
use prelay_server::{
    identity::credential::generate_credential,
    storage::{MasterKey, Storage},
};
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

async fn seed_owned_resources(
    storage: &Storage,
    pool: &SqlitePool,
    identity_id: &str,
    resource_suffix: &str,
) {
    let provider_id = storage
        .create_provider(
            identity_id,
            CreateProviderRequest {
                name: format!("Provider {resource_suffix}"),
                provider_type: "openai_compatible".to_string(),
                base_url: "https://provider.example".to_string(),
                api_key: "sk-secret".to_string(),
                capabilities: None,
                models: vec![format!("upstream-model-{resource_suffix}")],
            },
        )
        .await
        .expect("create provider");
    storage
        .create_interface(
            identity_id,
            CreateInterfaceRequest {
                name: format!("Interface {resource_suffix}"),
                protocol: None,
                models: vec![InterfaceModelInput {
                    model_name: Some(format!("model-{resource_suffix}")),
                    provider_id: provider_id.clone(),
                    upstream_model: format!("upstream-model-{resource_suffix}"),
                }],
            },
        )
        .await
        .expect("create interface");
    sqlx::query(
        "INSERT INTO identity_request_logs (id, identity_id, created_at, status) VALUES (?, ?, ?, ?)",
    )
    .bind(format!("log-{resource_suffix}"))
    .bind(identity_id)
    .bind(Utc::now().to_rfc3339())
    .bind("success")
    .execute(pool)
    .await
    .expect("create request log");
    sqlx::query(
        "INSERT INTO identity_response_sessions (response_id, identity_id, previous_response_id, provider_id, \
         model, input_messages_json, output_items_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(format!("response-{resource_suffix}"))
    .bind(identity_id)
    .bind(Option::<String>::None)
    .bind(&provider_id)
    .bind(format!("model-{resource_suffix}"))
    .bind("[]")
    .bind("[]")
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await
    .expect("create response session");
    sqlx::query(
        "INSERT INTO identity_model_aliases (id, identity_id, alias, provider_id, upstream_model, \
         downstream_protocols_json, enabled, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(format!("alias-{resource_suffix}"))
    .bind(identity_id)
    .bind(format!("alias-{resource_suffix}"))
    .bind(&provider_id)
    .bind(format!("upstream-model-{resource_suffix}"))
    .bind("[\"responses\"]")
    .bind(1)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await
    .expect("create model alias");
}

async fn owned_resource_count(pool: &SqlitePool, identity_id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT \
             (SELECT COUNT(*) FROM identity_provider_configs WHERE identity_id = ?) + \
             (SELECT COUNT(*) FROM identity_provider_models WHERE provider_id IN (\
                 SELECT id FROM identity_provider_configs WHERE identity_id = ?\
             )) + \
             (SELECT COUNT(*) FROM identity_interface_configs WHERE identity_id = ?) + \
             (SELECT COUNT(*) FROM identity_interface_models WHERE interface_id IN (\
                 SELECT id FROM identity_interface_configs WHERE identity_id = ?\
             )) + \
             (SELECT COUNT(*) FROM identity_response_sessions WHERE identity_id = ?) + \
             (SELECT COUNT(*) FROM identity_request_logs WHERE identity_id = ?) + \
             (SELECT COUNT(*) FROM identity_model_aliases WHERE identity_id = ?)",
    )
    .bind(identity_id)
    .bind(identity_id)
    .bind(identity_id)
    .bind(identity_id)
    .bind(identity_id)
    .bind(identity_id)
    .bind(identity_id)
    .fetch_one(pool)
    .await
    .expect("count identity owned resources")
}

#[tokio::test]
async fn cleanup_removes_inactive_identity_and_all_owned_data() {
    let (storage, pool) = test_storage().await;
    let identity = storage
        .register_identity("machine-a", "S-1-5-21-100", &generate_credential())
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
async fn cleanup_removes_identity_at_retention_cutoff_only() {
    let (storage, pool) = test_storage().await;
    let expires_at_cutoff = storage
        .register_identity("machine-at-cutoff", "S-1-5-21-101", &generate_credential())
        .await
        .expect("register identity at cutoff");
    let remains_active = storage
        .register_identity("machine-newer", "S-1-5-21-102", &generate_credential())
        .await
        .expect("register newer identity");
    let now = Utc
        .with_ymd_and_hms(2026, 4, 1, 0, 0, 0)
        .single()
        .expect("construct fixed current time");
    let cutoff = now - Duration::days(90);

    seed_owned_resources(&storage, &pool, &expires_at_cutoff.identity_id, "at-cutoff").await;
    seed_owned_resources(&storage, &pool, &remains_active.identity_id, "newer").await;

    for (identity_id, last_active_at) in [
        (&expires_at_cutoff.identity_id, cutoff),
        (&remains_active.identity_id, cutoff + Duration::seconds(1)),
    ] {
        sqlx::query("UPDATE identities SET last_active_at = ? WHERE id = ?")
            .bind(last_active_at.to_rfc3339())
            .bind(identity_id)
            .execute(&pool)
            .await
            .expect("set fixed last active time");
    }

    assert_eq!(
        storage
            .delete_inactive_identities(now, Duration::days(90))
            .await
            .expect("clean identities at retention boundary"),
        1
    );
    assert_eq!(
        owned_resource_count(&pool, &expires_at_cutoff.identity_id).await,
        0,
        "the cutoff identity must not retain child resources that block deletion"
    );
    assert_eq!(
        owned_resource_count(&pool, &remains_active.identity_id).await,
        7,
        "the newer identity must retain its child resources"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM identities WHERE id = ?")
            .bind(&expires_at_cutoff.identity_id)
            .fetch_one(&pool)
            .await
            .expect("check identity at cutoff"),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM identities WHERE id = ?")
            .bind(&remains_active.identity_id)
            .fetch_one(&pool)
            .await
            .expect("check newer identity"),
        1
    );
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
    for table in [
        "provider_models",
        "interface_configs",
        "interface_models",
        "response_sessions",
        "request_logs",
        "model_aliases",
    ] {
        sqlx::query(&format!("CREATE TABLE {table} (id TEXT PRIMARY KEY)"))
            .execute(&pool)
            .await
            .expect("create legacy business table");
        sqlx::query(&format!("INSERT INTO {table} (id) VALUES (?)"))
            .bind(format!("legacy-{table}"))
            .execute(&pool)
            .await
            .expect("store legacy business data");
    }
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
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN (\
         'provider_configs', 'provider_models', 'interface_configs', 'interface_models', \
         'response_sessions', 'request_logs', 'model_aliases')",
    )
    .fetch_one(&pool)
    .await
    .expect("inspect legacy tables");
    assert_eq!(legacy_table_count, 0);
    let identity_table_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN (\
         'identities', 'identity_provider_configs', 'identity_provider_models', \
         'identity_interface_configs', 'identity_interface_models', \
         'identity_response_sessions', 'identity_request_logs', 'identity_model_aliases')",
    )
    .fetch_one(&pool)
    .await
    .expect("inspect replacement identity tables");
    assert_eq!(identity_table_count, 8);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM identities")
            .fetch_one(&pool)
            .await
            .expect("count identities"),
        0
    );
}
