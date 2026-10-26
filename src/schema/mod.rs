mod activity_rollout;
mod anthropic_removal;
mod indexes;
mod provider_catalog;
mod reconciliation;
mod tables;
mod validation;

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    IntoActiveModel, TransactionTrait,
};
use sea_query::{IndexCreateStatement, TableCreateStatement};

use crate::entity::identity::provider_configs;

pub(crate) use validation::column_exists;
use validation::{current_schema_is_complete, incomplete_schema_error, table_count, table_exists};

const BASE_TABLES: [&str; 7] = [
    "identities",
    "identity_provider_configs",
    "identity_provider_shares",
    "identity_endpoint_configs",
    "identity_endpoint_models",
    "identity_endpoint_model_routes",
    "identity_response_sessions",
];
const LEGACY_ALIAS_TABLE: &str = "identity_model_aliases";
const LEGACY_ACTIVITY_TABLE: &str = "identity_request_logs";
const LEGACY_ACTIVITY_INDEX: &str = "idx_identity_request_logs_identity_created_at";
const MEMORY_TABLES: [&str; 3] = ["activity_rollouts", "memories", "memory_sources"];
const INCOMPLETE_SCHEMA_ERROR: &str =
    "database schema is incomplete; create a new database deployment";

pub async fn initialize(db: &DatabaseConnection) -> Result<(), DbErr> {
    let existing_base_tables = table_count(db, BASE_TABLES).await?;
    let has_current_activities = table_exists(db, "identity_activities").await?;
    let has_legacy_activities = table_exists(db, LEGACY_ACTIVITY_TABLE).await?;
    let has_activity_contents = table_exists(db, "activity_contents").await?;
    let has_activity_rollouts = table_exists(db, "activity_rollouts").await?;
    let has_memories = table_exists(db, "memories").await?;
    let has_memory_sources = table_exists(db, "memory_sources").await?;

    if existing_base_tables == 0
        && !has_current_activities
        && !has_legacy_activities
        && !has_activity_contents
        && !has_activity_rollouts
        && !has_memories
        && !has_memory_sources
    {
        let manager = SchemaInitializer { db };
        initialize_empty_schema(&manager).await?;
        activity_rollout::apply(db).await?;
        anthropic_removal::apply(db).await?;
        return Ok(());
    }
    if !validation::required_base_tables_exist(db).await? {
        return Err(incomplete_schema_error());
    }
    if !has_current_activities && !has_legacy_activities {
        return Err(incomplete_schema_error());
    }
    if has_memories != has_memory_sources
        || (!has_memories && (has_activity_contents || has_activity_rollouts))
    {
        return Err(incomplete_schema_error());
    }

    let transaction = db.begin().await?;
    let manager = SchemaInitializer { db: &transaction };
    manager.migrate_provider_schema().await?;
    manager.migrate_response_sessions_schema().await?;
    if !has_current_activities && has_legacy_activities {
        manager.rename_legacy_activities().await?;
    }
    manager.migrate_activity_schema().await?;
    manager.drop_legacy_cost_columns().await?;
    if !has_memories {
        initialize_memory_schema(&manager).await?;
    }
    activity_rollout::apply_in(&transaction).await?;
    anthropic_removal::apply_in(&transaction).await?;
    manager.validate_current_schema().await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn initialize_with_catalog(
    db: &DatabaseConnection,
    catalog: &crate::provider_catalog::ProviderCatalog,
) -> Result<(), DbErr> {
    initialize(db).await?;
    provider_catalog::apply(db, catalog).await?;
    backfill_activity_provider_types(db).await?;
    provider_catalog::migrate_provider_models(db, catalog).await?;
    reconciliation::reconcile_protocol_base_urls(db, catalog).await?;
    reconciliation::reconcile_endpoint_models(db, catalog).await
}

async fn initialize_empty_schema<C: ConnectionTrait>(
    manager: &SchemaInitializer<'_, C>,
) -> Result<(), DbErr> {
    manager.create_table(tables::identity::statement()).await?;
    manager.create_table(tables::providers::configs()).await?;
    manager
        .create_table(tables::provider_shares::statement())
        .await?;
    manager
        .create_index(indexes::provider_shares_provider_grantee())
        .await?;
    manager.create_table(tables::endpoints::configs()).await?;
    manager.create_table(tables::endpoints::models()).await?;
    manager.create_table(tables::endpoints::routes()).await?;
    manager.create_table(tables::sessions::statement()).await?;
    manager
        .create_table(tables::activities::statement())
        .await?;
    manager
        .create_index(indexes::activities_identity_created_at())
        .await?;
    initialize_memory_schema(manager).await
}

async fn initialize_memory_schema<C: ConnectionTrait>(
    manager: &SchemaInitializer<'_, C>,
) -> Result<(), DbErr> {
    manager
        .create_table(tables::activity_rollouts::statement())
        .await?;
    manager
        .create_index(indexes::activity_rollouts_path())
        .await?;
    manager
        .create_index(indexes::activity_rollouts_identity_created_at())
        .await?;
    manager
        .create_index(indexes::activity_rollouts_session())
        .await?;
    manager.create_table(tables::memories::statement()).await?;
    manager
        .create_index(indexes::memories_normalized_key())
        .await?;
    manager
        .create_table(tables::memory_sources::statement())
        .await?;
    manager
        .create_index(indexes::memory_sources_identity_observed_at())
        .await?;
    manager.create_index(indexes::memory_sources_unique()).await
}

struct SchemaInitializer<'a, C: ConnectionTrait> {
    db: &'a C,
}

impl<C: ConnectionTrait> SchemaInitializer<'_, C> {
    async fn create_table(&self, statement: TableCreateStatement) -> Result<(), DbErr> {
        self.db.execute(&statement).await.map(|_| ())
    }

    async fn create_index(&self, statement: IndexCreateStatement) -> Result<(), DbErr> {
        self.db.execute(&statement).await.map(|_| ())
    }

    async fn migrate_provider_schema(&self) -> Result<(), DbErr> {
        self.drop_legacy_alias_table().await?;
        // 只有真正当前的供应商表才做结构与数据迁移；表名齐全但结构不对的库留给校验报错。
        if !self.provider_table_is_current().await? {
            return Ok(());
        }
        if !column_exists(self.db, "identity_provider_configs", "visibility").await? {
            self.db
                .execute_unprepared(
                    "ALTER TABLE identity_provider_configs
                     ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'",
                )
                .await?;
        }
        if !column_exists(self.db, "identity_provider_configs", "models_json").await? {
            self.db
                .execute_unprepared(
                    "ALTER TABLE identity_provider_configs ADD COLUMN models_json TEXT",
                )
                .await?;
        }
        if !table_exists(self.db, "identity_provider_shares").await? {
            self.create_table(tables::provider_shares::statement())
                .await?;
            self.create_index(indexes::provider_shares_provider_grantee())
                .await?;
        }
        self.strip_legacy_protocol_overrides().await?;
        Ok(())
    }

    async fn migrate_response_sessions_schema(&self) -> Result<(), DbErr> {
        if !table_exists(self.db, "identity_response_sessions").await? {
            return Ok(());
        }
        if !column_exists(self.db, "identity_response_sessions", "conversation_id").await? {
            self.db
                .execute_unprepared(
                    "ALTER TABLE identity_response_sessions ADD COLUMN conversation_id TEXT",
                )
                .await?;
        }
        self.db
            .execute_unprepared(
                "UPDATE identity_response_sessions
                 SET conversation_id = response_id
                 WHERE conversation_id IS NULL",
            )
            .await?;
        Ok(())
    }

    async fn migrate_activity_schema(&self) -> Result<(), DbErr> {
        self.db
            .execute_unprepared(
                "ALTER TABLE identity_activities ADD COLUMN IF NOT EXISTS provider_type TEXT",
            )
            .await?;
        Ok(())
    }

    async fn provider_table_is_current(&self) -> Result<bool, DbErr> {
        for column in [
            "identity_id",
            "provider_type",
            "api_key_ciphertext",
            "created_at",
        ] {
            if !column_exists(self.db, "identity_provider_configs", column).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 模型别名表已退出请求链路，删除遗留表。
    async fn drop_legacy_alias_table(&self) -> Result<(), DbErr> {
        if table_exists(self.db, LEGACY_ALIAS_TABLE).await? {
            self.db
                .execute_unprepared(&format!("DROP TABLE {LEGACY_ALIAS_TABLE}"))
                .await?;
        }
        Ok(())
    }

    /// 协议集合改由目录决定，清掉供应商记录里遗留的协议集合覆盖；协议地址覆盖保留。
    async fn strip_legacy_protocol_overrides(&self) -> Result<(), DbErr> {
        for provider in provider_configs::Entity::find().all(self.db).await? {
            let Some(raw) = provider.capabilities_json.as_deref() else {
                continue;
            };
            let Ok(mut capabilities) = serde_json::from_str::<serde_json::Value>(raw) else {
                continue;
            };
            let Some(object) = capabilities.as_object_mut() else {
                continue;
            };
            if object.remove("upstream_protocols").is_none() {
                continue;
            }
            let updated = serde_json::to_string(&capabilities)
                .map_err(|error| DbErr::Custom(format!("serialize capabilities: {error}")))?;
            let mut active = provider.into_active_model();
            active.capabilities_json = Set(Some(updated));
            active.update(self.db).await?;
        }
        Ok(())
    }

    async fn validate_current_schema(&self) -> Result<(), DbErr> {
        if !current_schema_is_complete(self.db).await? {
            return Err(incomplete_schema_error());
        }
        Ok(())
    }

    async fn rename_legacy_activities(&self) -> Result<(), DbErr> {
        self.db
            .execute_unprepared(&format!("DROP INDEX IF EXISTS {LEGACY_ACTIVITY_INDEX}"))
            .await?;
        self.db
            .execute_unprepared(&format!(
                "ALTER TABLE {LEGACY_ACTIVITY_TABLE} RENAME TO identity_activities"
            ))
            .await?;
        self.create_index(indexes::activities_identity_created_at())
            .await
    }

    async fn drop_legacy_cost_columns(&self) -> Result<(), DbErr> {
        for column in ["estimated_cost", "currency"] {
            if column_exists(self.db, "identity_activities", column).await? {
                self.db
                    .execute_unprepared(&format!(
                        "ALTER TABLE identity_activities DROP COLUMN {column}"
                    ))
                    .await?;
            }
        }
        Ok(())
    }
}

/// 旧活动没有供应商类型：从仍存在的供应商记录按实例 ID 回填。
/// 供应商记录已删除的活动保持 NULL，统一归入“已删除供应商”。
pub(crate) async fn backfill_activity_provider_types(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        "UPDATE identity_activities AS activity
         SET provider_type = provider.provider_type
         FROM identity_provider_configs AS provider
         WHERE activity.provider_id = provider.id
           AND activity.provider_type IS NULL",
    )
    .await?;
    Ok(())
}
