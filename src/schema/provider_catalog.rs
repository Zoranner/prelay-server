use std::collections::HashMap;

use crate::{
    entity::identity::{endpoint_model_routes, endpoint_models, model_aliases, provider_configs},
    provider_catalog::ProviderCatalog,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection, DatabaseTransaction,
    DbBackend, DbErr, EntityTrait, IntoActiveModel, Statement, TransactionTrait,
};

const MIGRATION_VERSION: &str = "provider_catalog_v1";
const MODEL_SOURCE_MIGRATION_VERSION: &str = "provider_model_catalog_v2";

pub(crate) fn target_provider_id(provider_type: &str) -> Option<&'static str> {
    match provider_type.trim() {
        "gotoken" => Some("gotoken"),
        "deepseek" | "deepseek_anthropic" => Some("deepseek"),
        "kimi" | "kimi_coding" | "kimi_coding_anthropic" => Some("kimi"),
        "zhipu_coding" | "zhipu_coding_openai" => Some("bigmodel_coding_plan"),
        "bigmodel_coding_plan" => Some("bigmodel_coding_plan"),
        "qwen" | "qwen_token_plan" => Some("qwen_token_plan"),
        "minimax" | "minimax_token" | "minimax_token_openai" | "minimax_token_plan" => {
            Some("minimax")
        }
        _ => None,
    }
}

pub(crate) async fn apply(db: &DatabaseConnection, catalog: &ProviderCatalog) -> Result<(), DbErr> {
    if db.get_database_backend() != DbBackend::Postgres {
        return Ok(());
    }
    apply_postgres_legacy_migration(db, catalog).await?;
    apply_postgres_model_source_migration(db, catalog).await
}

async fn apply_postgres_legacy_migration(
    db: &DatabaseConnection,
    catalog: &ProviderCatalog,
) -> Result<(), DbErr> {
    let transaction = db.begin().await?;
    transaction
        .execute_unprepared(
            "CREATE TABLE IF NOT EXISTS prelay_schema_migrations (\
                version VARCHAR(128) PRIMARY KEY,\
                applied_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP\
            )",
        )
        .await?;

    let applied = transaction
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT version FROM prelay_schema_migrations WHERE version = $1",
            [MIGRATION_VERSION.into()],
        ))
        .await?
        .is_some();
    if applied {
        transaction.commit().await?;
        return Ok(());
    }

    let provider_rows = transaction
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id, provider_type FROM identity_provider_configs ORDER BY id".to_owned(),
        ))
        .await?;
    let mut provider_targets = HashMap::with_capacity(provider_rows.len());
    for row in provider_rows {
        let provider_id: String = row.try_get("", "id")?;
        let provider_type: String = row.try_get("", "provider_type")?;
        let target = target_provider_id(&provider_type).ok_or_else(|| {
            migration_error(format!(
                "供应商 {provider_id} 的旧 provider_type {provider_type} 没有明确目录映射"
            ))
        })?;
        if catalog.provider(target).is_none() {
            return Err(migration_error(format!(
                "供应商 {provider_id} 映射到目录供应商 {target}，但目录中不存在"
            )));
        }
        provider_targets.insert(provider_id, (provider_type, target));
    }

    cleanup_endpoint_models(&transaction, catalog, &provider_targets).await?;
    cleanup_model_aliases(&transaction, catalog, &provider_targets).await?;

    for (provider_id, (provider_type, target)) in &provider_targets {
        if provider_type != target {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "UPDATE identity_provider_configs SET provider_type = $1 WHERE id = $2",
                    [(*target).to_owned().into(), provider_id.clone().into()],
                ))
                .await?;
        }
    }
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO prelay_schema_migrations (version) VALUES ($1)",
            [MIGRATION_VERSION.into()],
        ))
        .await?;
    transaction.commit().await
}

async fn apply_postgres_model_source_migration(
    db: &DatabaseConnection,
    catalog: &ProviderCatalog,
) -> Result<(), DbErr> {
    let transaction = db.begin().await?;
    transaction
        .execute_unprepared(
            "CREATE TABLE IF NOT EXISTS prelay_schema_migrations (\
                version VARCHAR(128) PRIMARY KEY,\
                applied_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP\
            )",
        )
        .await?;

    let applied = transaction
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT version FROM prelay_schema_migrations WHERE version = $1",
            [MODEL_SOURCE_MIGRATION_VERSION.into()],
        ))
        .await?
        .is_some();
    if applied {
        transaction.commit().await?;
        return Ok(());
    }

    let provider_rows = provider_configs::Entity::find().all(&transaction).await?;
    let mut provider_targets = HashMap::with_capacity(provider_rows.len());
    for provider in provider_rows {
        let target = target_provider_id(&provider.provider_type).ok_or_else(|| {
            migration_error(format!(
                "供应商 {} 的 provider_type {} 没有明确目录映射",
                provider.id, provider.provider_type
            ))
        })?;
        if catalog.provider(target).is_none() {
            return Err(migration_error(format!(
                "供应商 {} 映射到目录供应商 {}，但目录中不存在",
                provider.id, target
            )));
        }
        let provider_id = provider.id.clone();
        if provider.provider_type != target {
            let mut active = provider.into_active_model();
            active.provider_type = Set(target.to_string());
            active.update(&transaction).await?;
        }
        provider_targets.insert(provider_id, target);
    }

    let endpoint_rows = endpoint_models::Entity::find().all(&transaction).await?;
    for endpoint_model in endpoint_rows {
        let target = provider_targets
            .get(&endpoint_model.provider_id)
            .copied()
            .ok_or_else(|| {
                migration_error(format!(
                    "接入点模型 {} 引用了不存在的供应商 {}",
                    endpoint_model.id, endpoint_model.provider_id
                ))
            })?;
        if endpoint_model.model_name != endpoint_model.upstream_model
            || should_remove_model(catalog, target, &endpoint_model.upstream_model)
        {
            endpoint_model_routes::Entity::delete_by_id((
                endpoint_model.endpoint_id.clone(),
                endpoint_model.model_name.clone(),
            ))
            .exec(&transaction)
            .await?;
            endpoint_models::Entity::delete_by_id(endpoint_model.id)
                .exec(&transaction)
                .await?;
        }
    }

    let aliases = model_aliases::Entity::find().all(&transaction).await?;
    for alias in aliases {
        let target = provider_targets
            .get(&alias.provider_id)
            .copied()
            .ok_or_else(|| {
                migration_error(format!(
                    "模型别名 {} 引用了不存在的供应商 {}",
                    alias.id, alias.provider_id
                ))
            })?;
        if should_remove_model(catalog, target, &alias.upstream_model) {
            model_aliases::Entity::delete_by_id(alias.id)
                .exec(&transaction)
                .await?;
        }
    }

    transaction
        .execute_unprepared("DROP TABLE IF EXISTS identity_provider_models")
        .await?;
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO prelay_schema_migrations (version) VALUES ($1)",
            [MODEL_SOURCE_MIGRATION_VERSION.into()],
        ))
        .await?;
    transaction.commit().await
}

async fn cleanup_endpoint_models(
    transaction: &DatabaseTransaction,
    catalog: &ProviderCatalog,
    provider_targets: &HashMap<String, (String, &'static str)>,
) -> Result<(), DbErr> {
    let rows = transaction
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id, endpoint_id, provider_id, model_name, upstream_model FROM identity_endpoint_models ORDER BY id"
                .to_owned(),
        ))
        .await?;
    for row in rows {
        let endpoint_model_id: String = row.try_get("", "id")?;
        let provider_id: String = row.try_get("", "provider_id")?;
        let model_name: String = row.try_get("", "model_name")?;
        let upstream_model: String = row.try_get("", "upstream_model")?;
        let target = provider_target(&provider_id, provider_targets)?;
        if model_name.trim() != upstream_model.trim()
            || should_remove_model(catalog, target, &upstream_model)
        {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "DELETE FROM identity_endpoint_model_routes WHERE endpoint_id = $1 AND model_name = $2 AND provider_id = $3",
                    [
                        row.try_get::<String>("", "endpoint_id")?.into(),
                        model_name.clone().into(),
                        provider_id.clone().into(),
                    ],
                ))
                .await?;
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "DELETE FROM identity_endpoint_models WHERE id = $1",
                    [endpoint_model_id.into()],
                ))
                .await?;
        }
    }
    Ok(())
}

async fn cleanup_model_aliases(
    transaction: &DatabaseTransaction,
    catalog: &ProviderCatalog,
    provider_targets: &HashMap<String, (String, &'static str)>,
) -> Result<(), DbErr> {
    let rows = transaction
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id, provider_id, upstream_model FROM identity_model_aliases ORDER BY id"
                .to_owned(),
        ))
        .await?;
    for row in rows {
        let alias_id: String = row.try_get("", "id")?;
        let provider_id: String = row.try_get("", "provider_id")?;
        let upstream_model: String = row.try_get("", "upstream_model")?;
        let target = provider_target(&provider_id, provider_targets)?;
        if should_remove_model(catalog, target, &upstream_model) {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "DELETE FROM identity_model_aliases WHERE id = $1",
                    [alias_id.into()],
                ))
                .await?;
        }
    }
    Ok(())
}

fn should_remove_model(catalog: &ProviderCatalog, provider_id: &str, model_name: &str) -> bool {
    !catalog.provider_supports_language_model(provider_id, model_name)
        && !catalog.provider_supports_image_generation_model(provider_id, model_name)
}

fn provider_target(
    provider_id: &str,
    provider_targets: &HashMap<String, (String, &'static str)>,
) -> Result<&'static str, DbErr> {
    provider_targets
        .get(provider_id)
        .map(|(_, target)| *target)
        .ok_or_else(|| migration_error(format!("数据引用了不存在的供应商 {provider_id}")))
}

fn migration_error(message: impl Into<String>) -> DbErr {
    DbErr::Custom(format!(
        "provider catalog migration failed: {}",
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::{should_remove_model, target_provider_id};
    use crate::provider_catalog::ProviderCatalog;
    use std::path::Path;

    #[test]
    fn marks_models_outside_provider_catalog_for_cleanup() {
        let catalog =
            ProviderCatalog::load(Path::new("config/catalog")).expect("load provider catalog");

        assert!(should_remove_model(
            &catalog,
            "gotoken",
            "codex-auto-review"
        ));
        assert!(!should_remove_model(&catalog, "gotoken", "gpt-5.6-sol"));
    }

    #[test]
    fn maps_legacy_provider_types_to_current_catalog_ids() {
        assert_eq!(target_provider_id("gotoken"), Some("gotoken"));
        assert_eq!(target_provider_id("kimi_coding_anthropic"), Some("kimi"));
        assert_eq!(
            target_provider_id("zhipu_coding"),
            Some("bigmodel_coding_plan")
        );
        assert_eq!(target_provider_id("minimax_token"), Some("minimax"));
        assert_eq!(target_provider_id("qwen"), Some("qwen_token_plan"));
    }

    #[test]
    fn rejects_provider_types_without_an_unambiguous_catalog_mapping() {
        assert_eq!(target_provider_id("zhipu"), None);
        assert_eq!(target_provider_id("custom_provider"), None);
    }
}
