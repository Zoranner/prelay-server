use std::collections::HashMap;

use crate::provider_catalog::ProviderCatalog;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, Statement,
    TransactionTrait,
};

const MIGRATION_VERSION: &str = "provider_catalog_v1";

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

    validate_provider_models(&transaction, catalog, &provider_targets).await?;
    validate_endpoint_models(&transaction, catalog, &provider_targets).await?;
    validate_model_aliases(&transaction, catalog, &provider_targets).await?;

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

async fn validate_provider_models(
    transaction: &DatabaseTransaction,
    catalog: &ProviderCatalog,
    provider_targets: &HashMap<String, (String, &'static str)>,
) -> Result<(), DbErr> {
    let rows = transaction
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT provider_id, model_name FROM identity_provider_models ORDER BY provider_id, model_name"
                .to_owned(),
        ))
        .await?;
    for row in rows {
        let provider_id: String = row.try_get("", "provider_id")?;
        let model_name: String = row.try_get("", "model_name")?;
        let target = provider_target(&provider_id, provider_targets)?;
        if !catalog.provider_supports_language_model(target, &model_name)
            && !catalog.provider_supports_image_generation_model(target, &model_name)
        {
            return Err(migration_error(format!(
                "供应商 {provider_id} 的模型 {model_name} 不在目录供应商 {target} 的模型清单中"
            )));
        }
    }
    Ok(())
}

async fn validate_endpoint_models(
    transaction: &DatabaseTransaction,
    catalog: &ProviderCatalog,
    provider_targets: &HashMap<String, (String, &'static str)>,
) -> Result<(), DbErr> {
    let rows = transaction
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id, provider_id, model_name, upstream_model FROM identity_endpoint_models ORDER BY id"
                .to_owned(),
        ))
        .await?;
    for row in rows {
        let endpoint_model_id: String = row.try_get("", "id")?;
        let provider_id: String = row.try_get("", "provider_id")?;
        let model_name: String = row.try_get("", "model_name")?;
        let upstream_model: String = row.try_get("", "upstream_model")?;
        if model_name.trim() != upstream_model.trim() {
            return Err(migration_error(format!(
                "接入点模型 {endpoint_model_id} 使用旧别名 {model_name} -> {upstream_model}，无法无损迁移"
            )));
        }
        let target = provider_target(&provider_id, provider_targets)?;
        if !catalog.provider_supports_language_model(target, &upstream_model)
            && !catalog.provider_supports_image_generation_model(target, &upstream_model)
        {
            return Err(migration_error(format!(
                "接入点模型 {endpoint_model_id} 的上游模型 {upstream_model} 不在目录供应商 {target} 的模型清单中"
            )));
        }
    }
    Ok(())
}

async fn validate_model_aliases(
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
        if !catalog.provider_supports_language_model(target, &upstream_model)
            && !catalog.provider_supports_image_generation_model(target, &upstream_model)
        {
            return Err(migration_error(format!(
                "模型别名 {alias_id} 的上游模型 {upstream_model} 不在目录供应商 {target} 的模型清单中"
            )));
        }
    }
    Ok(())
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
    use super::target_provider_id;

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
