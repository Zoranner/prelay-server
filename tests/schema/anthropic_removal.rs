use prelay_server::{
    schema::initialize_with_catalog,
    test_support::{fixture_catalog, test_empty_database_connection},
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Postgres, sql.to_owned()))
        .await
        .expect("query count")
        .expect("count row");
    row.try_get("", "result_count").expect("count column")
}

async fn seed_identity(db: &DatabaseConnection, id: &str, machine: &str) {
    db.execute_unprepared(&format!(
        "INSERT INTO identities
         (id, machine_id, account_sid, credential_hash, display_name, created_at, last_active_at)
         VALUES ('{id}', '{machine}', 'S-1-5-21-{machine}', 'hash', '',
         '2026-10-23T00:00:00Z', '2026-10-23T00:00:00Z')"
    ))
    .await
    .expect("seed identity");
}

async fn seed_endpoint(db: &DatabaseConnection, id: &str, identity_id: &str) {
    db.execute_unprepared(&format!(
        "INSERT INTO identity_endpoint_configs
         (id, identity_id, name, protocol, token, created_at)
         VALUES ('{id}', '{identity_id}', 'Endpoint', 'all', 'token-{id}',
         '2026-10-23T00:00:00Z')"
    ))
    .await
    .expect("seed endpoint");
}

async fn seed_provider(
    db: &DatabaseConnection,
    id: &str,
    identity_id: &str,
    provider_type: &str,
    capabilities_json: &str,
    models_json: &str,
) {
    db.execute_unprepared(&format!(
        "INSERT INTO identity_provider_configs
         (id, identity_id, name, provider_type, visibility, base_url,
          api_key_ciphertext, capabilities_json, models_json, created_at)
         VALUES ('{id}', '{identity_id}', 'Provider', '{provider_type}', 'private',
          'https://provider.example/v1', 'ciphertext', '{capabilities_json}',
          '{models_json}', '2026-10-23T00:00:00Z')"
    ))
    .await
    .expect("seed provider");
}

#[tokio::test]
async fn removes_anthropic_protocol_data_during_schema_initialization() {
    let db = test_empty_database_connection().await;
    let catalog = fixture_catalog();
    initialize_with_catalog(&db, &catalog)
        .await
        .expect("initialize schema");

    // 模拟已经在旧版本启动过的数据库：迁移版本存在，但仍有协议残留数据。
    db.execute_unprepared(
        "DELETE FROM prelay_schema_migrations WHERE version = 'anthropic_removal_v1'",
    )
    .await
    .expect("remove migration marker");
    seed_identity(&db, "identity-a", "a").await;
    seed_identity(&db, "identity-b", "b").await;
    seed_endpoint(&db, "endpoint-a", "identity-a").await;
    seed_provider(
        &db,
        "provider-anthropic",
        "identity-a",
        "anthropic_compatible",
        r#"{"protocol_base_urls":{"anthropic":"https://old.example/v1","openai":"https://keep.example/v1"},"upstream_protocols":["anthropic","openai"]}"#,
        r#"["deepseek-v4-pro"]"#,
    )
    .await;
    seed_provider(
        &db,
        "provider-alias",
        "identity-a",
        "deepseek_anthropic",
        r#"{"protocol_base_urls":{"anthropic":"https://old.example/anthropic"}}"#,
        r#"["deepseek-v4-pro"]"#,
    )
    .await;
    seed_provider(
        &db,
        "provider-keep",
        "identity-a",
        "gotoken",
        r#"{"protocol_base_urls":{"anthropic":"https://old.example/v1","openai":"https://keep.example/v1"},"upstream_protocols":["anthropic","openai"]}"#,
        r#"["gpt-6-astra"]"#,
    )
    .await;

    db.execute_unprepared(
        "INSERT INTO identity_provider_shares (provider_id, grantee_identity_id, created_at)
         VALUES ('provider-anthropic', 'identity-b', '2026-10-23T00:00:00Z')",
    )
    .await
    .expect("seed provider share");
    db.execute_unprepared(
        "INSERT INTO identity_endpoint_models
         (id, endpoint_id, provider_id, model_name, upstream_model, candidate_order, created_at)
         VALUES ('model-anthropic', 'endpoint-a', 'provider-anthropic',
         'deepseek-v4-pro', 'deepseek-v4-pro', 0, '2026-10-23T00:00:00Z')",
    )
    .await
    .expect("seed endpoint model");
    db.execute_unprepared(
        "INSERT INTO identity_endpoint_model_routes
         (endpoint_id, model_name, provider_id, updated_at)
         VALUES ('endpoint-a', 'deepseek-v4-pro', 'provider-anthropic',
         '2026-10-23T00:00:00Z')",
    )
    .await
    .expect("seed endpoint route");
    db.execute_unprepared(
        "INSERT INTO identity_activities
         (id, identity_id, created_at, protocol_in, protocol_out, protocol_upstream, status)
         VALUES ('activity-anthropic', 'identity-a', '2026-10-23T00:00:00Z',
         'anthropic_messages', 'anthropic_messages', 'anthropic_messages', 'success'),
         ('activity-keep', 'identity-a', '2026-10-23T00:00:01Z',
         'responses', 'responses', 'chat_completions', 'success')",
    )
    .await
    .expect("seed activities");
    db.execute_unprepared(
        "INSERT INTO activity_rollouts
         (activity_id, identity_id, session_id, rollout_path, status,
          event_count, byte_size, created_at)
         VALUES ('activity-anthropic', 'identity-a', 'session-a',
         '2026/10/23/rollout-anthropic.jsonl', 'completed', 0, 0,
         '2026-10-23T00:00:00Z')",
    )
    .await
    .expect("seed rollout");

    initialize_with_catalog(&db, &catalog)
        .await
        .expect("apply protocol cleanup");

    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM identity_provider_configs
             WHERE id = 'provider-anthropic'"
        )
        .await,
        0,
        "Anthropic-only provider must be removed"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM identity_endpoint_models
             WHERE id = 'model-anthropic'"
        )
        .await,
        0,
        "endpoint model of the removed provider must be removed"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM identity_endpoint_model_routes
             WHERE provider_id = 'provider-anthropic'"
        )
        .await,
        0,
        "endpoint route of the removed provider must be removed"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM identity_provider_shares
             WHERE provider_id = 'provider-anthropic'"
        )
        .await,
        0,
        "provider share of the removed provider must be removed"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM identity_provider_configs
             WHERE id = 'provider-alias' AND provider_type = 'deepseek'"
        )
        .await,
        1,
        "known Anthropic alias must be normalized"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM identity_provider_configs
             WHERE id = 'provider-keep'"
        )
        .await,
        1,
        "other providers must remain"
    );
    let capabilities: serde_json::Value = {
        let row = db
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT capabilities_json FROM identity_provider_configs
                 WHERE id = 'provider-keep'"
                    .to_owned(),
            ))
            .await
            .expect("load capabilities")
            .expect("provider row");
        serde_json::from_str(
            row.try_get::<Option<String>>("", "capabilities_json")
                .expect("capabilities column")
                .as_deref()
                .expect("capabilities json"),
        )
        .expect("parse capabilities")
    };
    assert!(capabilities["protocol_base_urls"]
        .get("anthropic")
        .is_none());
    assert_eq!(
        capabilities["protocol_base_urls"]["openai"],
        serde_json::json!("https://keep.example/v1")
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM identity_activities
             WHERE id = 'activity-anthropic'"
        )
        .await,
        0,
        "Anthropic activity must be removed"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM identity_activities
             WHERE id = 'activity-keep'"
        )
        .await,
        1,
        "other activities must remain"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM activity_rollouts
             WHERE activity_id = 'activity-anthropic'"
        )
        .await,
        0,
        "rollout index of the removed activity must be removed"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM prelay_schema_migrations
             WHERE version = 'anthropic_removal_v1'"
        )
        .await,
        1
    );

    initialize_with_catalog(&db, &catalog)
        .await
        .expect("cleanup migration is idempotent");
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) AS result_count FROM prelay_schema_migrations
             WHERE version = 'anthropic_removal_v1'"
        )
        .await,
        1
    );
}
