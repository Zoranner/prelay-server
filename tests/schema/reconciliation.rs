use prelay_server::{
    entity::identity::{endpoint_model_routes, endpoint_models, provider_configs},
    schema::{initialize, initialize_with_catalog},
    test_support::fixture_catalog,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    DbBackend, EntityTrait, QueryFilter, Statement,
};

async fn connect() -> DatabaseConnection {
    prelay_server::test_support::test_empty_database_connection().await
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
        .expect("inspect database schema")
        .expect("table count row");
    row.try_get::<i64>("", "result_count").unwrap() == 1
}

async fn column_exists(db: &DatabaseConnection, table: &str, column: &str) -> bool {
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
        .expect("inspect database schema")
        .expect("column count row");
    row.try_get::<i64>("", "result_count").unwrap() == 1
}

async fn insert_identity(db: &DatabaseConnection) {
    db.execute_unprepared(
        "INSERT INTO identities \
         (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at) \
         VALUES ('identity-a', 'machine-a', 'S-1-5-21-100', 'hash', '', \
         '2026-09-14T00:00:00Z', '2026-09-14T00:00:00Z')",
    )
    .await
    .expect("insert identity");
}

async fn insert_endpoint(db: &DatabaseConnection) {
    db.execute_unprepared(
        "INSERT INTO identity_endpoint_configs \
         (id, identity_id, name, protocol, token, created_at) \
         VALUES ('endpoint-a', 'identity-a', 'Endpoint', 'all', 'token-a', '2026-09-14T00:00:00Z')",
    )
    .await
    .expect("insert endpoint");
}

async fn insert_provider(
    db: &DatabaseConnection,
    id: &str,
    provider_type: &str,
    capabilities_json: Option<String>,
    models_json: Option<String>,
) {
    provider_configs::ActiveModel {
        id: Set(id.to_string()),
        identity_id: Set("identity-a".to_string()),
        name: Set("Provider".to_string()),
        provider_type: Set(provider_type.to_string()),
        visibility: Set("private".to_string()),
        base_url: Set("https://provider.example/v1".to_string()),
        api_key_ciphertext: Set("ciphertext".to_string()),
        capabilities_json: Set(capabilities_json),
        models_json: Set(models_json),
        created_at: Set("2026-09-14T00:00:00Z".to_string()),
    }
    .insert(db)
    .await
    .expect("insert provider");
}

async fn insert_endpoint_model(
    db: &DatabaseConnection,
    id: &str,
    provider_id: &str,
    model_name: &str,
    upstream_model: &str,
) {
    endpoint_models::ActiveModel {
        id: Set(id.to_string()),
        endpoint_id: Set("endpoint-a".to_string()),
        provider_id: Set(provider_id.to_string()),
        model_name: Set(model_name.to_string()),
        upstream_model: Set(upstream_model.to_string()),
        candidate_order: Set(0),
        created_at: Set("2026-09-14T00:00:00Z".to_string()),
    }
    .insert(db)
    .await
    .expect("insert endpoint model");
}

async fn insert_route(db: &DatabaseConnection, model_name: &str, provider_id: &str) {
    endpoint_model_routes::ActiveModel {
        endpoint_id: Set("endpoint-a".to_string()),
        model_name: Set(model_name.to_string()),
        provider_id: Set(provider_id.to_string()),
        updated_at: Set("2026-09-14T00:00:00Z".to_string()),
    }
    .insert(db)
    .await
    .expect("insert endpoint route");
}

#[tokio::test]
async fn migrates_legacy_provider_columns_on_startup() {
    let db = connect().await;
    initialize(&db).await.expect("initialize schema");
    insert_identity(&db).await;
    db.execute_unprepared("CREATE TABLE identity_model_aliases (id TEXT PRIMARY KEY)")
        .await
        .expect("create legacy alias table");
    db.execute_unprepared(
        "ALTER TABLE identity_provider_configs ADD COLUMN disabled_models_json TEXT",
    )
    .await
    .expect("add legacy disabled models column");
    insert_provider(
        &db,
        "provider-a",
        "gotoken",
        Some(
            r#"{"upstream_protocols":["openai"],"protocol_base_urls":{"openai":"https://gateway.example/v1"}}"#
                .to_string(),
        ),
        None,
    )
    .await;
    db.execute_unprepared(
        "UPDATE identity_provider_configs SET disabled_models_json = '[\"gpt-5.6-sol\"]'",
    )
    .await
    .expect("store legacy disabled models");

    initialize_with_catalog(&db, &fixture_catalog())
        .await
        .expect("migrate existing schema");

    assert!(
        !table_exists(&db, "identity_model_aliases").await,
        "legacy alias table must be dropped"
    );
    assert!(
        !column_exists(&db, "identity_provider_configs", "disabled_models_json").await,
        "legacy disabled models column must be dropped"
    );
    let provider = provider_configs::Entity::find_by_id("provider-a")
        .one(&db)
        .await
        .expect("load provider")
        .expect("provider row");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(provider.models_json.as_deref().expect("models json"))
            .expect("parse models"),
        vec![
            "gpt-5.6-luna".to_string(),
            "gpt-5.6-terra".to_string(),
            "gpt-6-astra".to_string(),
            "gpt-image-1".to_string(),
        ],
        "enabled models come from the catalog entry minus the legacy disabled list"
    );
    let capabilities: serde_json::Value = serde_json::from_str(
        provider
            .capabilities_json
            .as_deref()
            .expect("capabilities json"),
    )
    .expect("parse capabilities");
    assert!(
        capabilities.get("upstream_protocols").is_none(),
        "protocol set overrides must be removed"
    );
    assert_eq!(
        capabilities["protocol_base_urls"]["openai"],
        serde_json::json!("https://gateway.example/v1")
    );
}

#[tokio::test]
async fn reconciles_endpoint_models_with_the_current_catalog() {
    let db = connect().await;
    // 第一次初始化记录目录迁移版本，第二次只跑接入点模型对账。
    initialize_with_catalog(&db, &fixture_catalog())
        .await
        .expect("initialize schema and catalog");
    insert_identity(&db).await;
    insert_endpoint(&db).await;
    insert_provider(
        &db,
        "provider-relay",
        "relay",
        None,
        Some(r#"["k3"]"#.to_string()),
    )
    .await;
    insert_endpoint_model(&db, "model-mapped", "provider-relay", "k3", "k3").await;
    insert_endpoint_model(
        &db,
        "model-unsupported",
        "provider-relay",
        "gpt-5.6-sol",
        "gpt-5.6-sol",
    )
    .await;
    insert_route(&db, "gpt-5.6-sol", "provider-relay").await;

    initialize_with_catalog(&db, &fixture_catalog())
        .await
        .expect("reconcile endpoint models");

    let mapped = endpoint_models::Entity::find_by_id("model-mapped")
        .one(&db)
        .await
        .expect("load mapped model")
        .expect("mapped model row");
    assert_eq!(mapped.upstream_model, "kimi-k3");

    assert!(
        endpoint_models::Entity::find_by_id("model-unsupported")
            .one(&db)
            .await
            .expect("load unsupported model")
            .is_none(),
        "model outside the enabled list must be removed"
    );
    assert!(
        endpoint_model_routes::Entity::find()
            .filter(endpoint_model_routes::Column::ModelName.eq("gpt-5.6-sol"))
            .all(&db)
            .await
            .expect("load routes")
            .is_empty(),
        "routes of removed models must be removed"
    );
}
