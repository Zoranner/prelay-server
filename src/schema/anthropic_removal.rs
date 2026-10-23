use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    IntoActiveModel, Statement, TransactionTrait,
};
use serde_json::Value;

use crate::entity::identity::provider_configs;

const MIGRATION_VERSION: &str = "anthropic_removal_v1";

/// 旧 provider_type 是 Anthropic 兼容端点变体；能落到当前目录条目的做归一化，
/// 没有对应目录条目的纯 Anthropic 供应商记录在迁移中删除。
const PROVIDER_TYPE_ALIASES: [(&str, &str); 4] = [
    ("deepseek_anthropic", "deepseek"),
    ("kimi_coding_anthropic", "kimi"),
    ("zhipu_coding", "bigmodel_coding_plan"),
    ("minimax_token", "minimax"),
];

const ANTHROPIC_ONLY_PROVIDER_TYPES: [&str; 8] = [
    "anthropic",
    "anthropic_compatible",
    "qwen_anthropic",
    "zhipu_anthropic",
    "minimax_anthropic",
    "zai_coding_anthropic",
    "bailian_coding_anthropic",
    "bailian_token_anthropic",
];

pub(super) async fn apply(db: &DatabaseConnection) -> Result<(), DbErr> {
    let transaction = db.begin().await?;
    apply_in(&transaction).await?;
    transaction.commit().await
}

pub(super) async fn apply_in<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS prelay_schema_migrations (
            version VARCHAR(128) PRIMARY KEY,
            applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .await?;

    let applied = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT version FROM prelay_schema_migrations WHERE version = $1",
            [MIGRATION_VERSION.into()],
        ))
        .await?
        .is_some();
    if applied {
        return Ok(());
    }

    delete_anthropic_activities(db).await?;
    clean_provider_records(db).await?;

    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO prelay_schema_migrations (version) VALUES ($1) ON CONFLICT (version) DO NOTHING",
        [MIGRATION_VERSION.into()],
    ))
    .await?;
    tracing::info!(version = MIGRATION_VERSION, "applied schema migration");
    Ok(())
}

async fn delete_anthropic_activities<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let mut conditions = Vec::new();
    for column in ["protocol_in", "protocol_out", "protocol_upstream"] {
        if super::column_exists(db, "identity_activities", column).await? {
            conditions.push(format!("{column} IN ('anthropic', 'anthropic_messages')"));
        }
    }
    if conditions.is_empty() {
        return Ok(());
    }
    let predicate = conditions.join(" OR ");

    if super::column_exists(db, "activity_rollouts", "activity_id").await? {
        db.execute_unprepared(&format!(
            "DELETE FROM activity_rollouts WHERE activity_id IN (
                SELECT id FROM identity_activities WHERE {predicate}
            )"
        ))
        .await?;
    }
    db.execute_unprepared(&format!(
        "DELETE FROM identity_activities WHERE {predicate}"
    ))
    .await?;
    Ok(())
}

async fn clean_provider_records<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    if !provider_table_is_readable(db).await? {
        return Ok(());
    }

    for (from, to) in PROVIDER_TYPE_ALIASES {
        db.execute_unprepared(&format!(
            "UPDATE identity_provider_configs
             SET provider_type = '{to}'
             WHERE provider_type = '{from}'"
        ))
        .await?;
    }

    let provider_types = ANTHROPIC_ONLY_PROVIDER_TYPES
        .iter()
        .map(|provider_type| format!("'{provider_type}'"))
        .collect::<Vec<_>>()
        .join(", ");
    for table in [
        "identity_provider_shares",
        "identity_endpoint_model_routes",
        "identity_endpoint_models",
    ] {
        if super::column_exists(db, table, "provider_id").await? {
            db.execute_unprepared(&format!(
                "DELETE FROM {table}
                 WHERE provider_id IN (
                     SELECT id FROM identity_provider_configs
                     WHERE provider_type IN ({provider_types})
                 )"
            ))
            .await?;
        }
    }
    db.execute_unprepared(&format!(
        "DELETE FROM identity_provider_configs
         WHERE provider_type IN ({provider_types})"
    ))
    .await?;

    strip_anthropic_capabilities(db).await
}

async fn strip_anthropic_capabilities<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    for provider in provider_configs::Entity::find().all(db).await? {
        let Some(raw) = provider.capabilities_json.as_deref() else {
            continue;
        };
        let Ok(mut capabilities) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        let Some(object) = capabilities.as_object_mut() else {
            continue;
        };
        let mut changed = false;

        if let Some(base_urls) = object
            .get_mut("protocol_base_urls")
            .and_then(Value::as_object_mut)
        {
            changed |= base_urls.remove("anthropic").is_some();
            changed |= base_urls.remove("anthropic_messages").is_some();
        }
        if let Some(protocols) = object
            .get_mut("upstream_protocols")
            .and_then(Value::as_array_mut)
        {
            let before = protocols.len();
            protocols.retain(|value| {
                !matches!(
                    value.as_str(),
                    Some("anthropic") | Some("anthropic_messages")
                )
            });
            changed |= protocols.len() != before;
        }
        if !changed {
            continue;
        }

        let updated = serde_json::to_string(&capabilities)
            .map_err(|error| DbErr::Custom(format!("serialize capabilities: {error}")))?;
        let mut active = provider.into_active_model();
        active.capabilities_json = sea_orm::ActiveValue::Set(Some(updated));
        active.update(db).await?;
    }
    Ok(())
}

async fn provider_table_is_readable<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
    for column in [
        "id",
        "identity_id",
        "name",
        "provider_type",
        "visibility",
        "base_url",
        "api_key_ciphertext",
        "capabilities_json",
        "models_json",
        "created_at",
    ] {
        if !super::column_exists(db, "identity_provider_configs", column).await? {
            return Ok(false);
        }
    }
    Ok(true)
}
