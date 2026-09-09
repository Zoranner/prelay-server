mod activity_content;
mod indexes;
mod provider_catalog;
mod tables;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, TransactionTrait};
use sea_query::{IndexCreateStatement, TableCreateStatement};

const BASE_TABLES: [&str; 8] = [
    "identities",
    "identity_provider_configs",
    "identity_provider_shares",
    "identity_endpoint_configs",
    "identity_endpoint_models",
    "identity_endpoint_model_routes",
    "identity_response_sessions",
    "identity_model_aliases",
];
const LEGACY_ACTIVITY_TABLE: &str = "identity_request_logs";
const LEGACY_ACTIVITY_INDEX: &str = "idx_identity_request_logs_identity_created_at";
const MEMORY_TABLES: [&str; 3] = ["activity_contents", "memories", "memory_sources"];
const INCOMPLETE_SCHEMA_ERROR: &str =
    "database schema is incomplete; create a new database deployment";

pub async fn initialize(db: &DatabaseConnection) -> Result<(), DbErr> {
    let existing_base_tables = table_count(db, BASE_TABLES).await?;
    let has_current_activities = table_exists(db, "identity_activities").await?;
    let has_legacy_activities = table_exists(db, LEGACY_ACTIVITY_TABLE).await?;
    let existing_memory_tables = table_count(db, MEMORY_TABLES).await?;

    if existing_base_tables == 0
        && !has_current_activities
        && !has_legacy_activities
        && existing_memory_tables == 0
    {
        let manager = SchemaInitializer { db };
        initialize_empty_schema(&manager).await?;
        activity_content::apply(db).await?;
        return Ok(());
    }
    if !required_base_tables_exist(db).await? {
        return Err(incomplete_schema_error());
    }
    if !has_current_activities && !has_legacy_activities {
        return Err(incomplete_schema_error());
    }

    let transaction = db.begin().await?;
    let manager = SchemaInitializer { db: &transaction };
    manager.migrate_provider_sharing_schema().await?;
    if !has_current_activities && has_legacy_activities {
        manager.rename_legacy_activities().await?;
    }
    manager.drop_legacy_cost_columns().await?;
    match existing_memory_tables {
        0 => initialize_memory_schema(&manager).await?,
        count if count == MEMORY_TABLES.len() => {}
        _ => return Err(incomplete_schema_error()),
    }
    manager.validate_current_schema().await?;
    transaction.commit().await?;
    activity_content::apply(db).await
}

pub async fn initialize_with_catalog(
    db: &DatabaseConnection,
    catalog: &crate::provider_catalog::ProviderCatalog,
) -> Result<(), DbErr> {
    initialize(db).await?;
    provider_catalog::apply(db, catalog).await
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
    manager
        .create_table(tables::model_aliases::statement())
        .await?;
    initialize_memory_schema(manager).await
}

async fn initialize_memory_schema<C: ConnectionTrait>(
    manager: &SchemaInitializer<'_, C>,
) -> Result<(), DbErr> {
    manager
        .create_table(tables::activity_contents::statement())
        .await?;
    manager
        .create_index(indexes::activity_contents_activity_id())
        .await?;
    manager
        .create_index(indexes::activity_contents_due())
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

async fn table_count<'a, C: ConnectionTrait>(
    db: &C,
    tables: impl IntoIterator<Item = &'a str>,
) -> Result<usize, DbErr> {
    let mut count = 0;
    for table in tables {
        count += usize::from(table_exists(db, table).await?);
    }
    Ok(count)
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

    async fn migrate_provider_sharing_schema(&self) -> Result<(), DbErr> {
        if !column_exists(self.db, "identity_provider_configs", "visibility").await? {
            self.db
                .execute_unprepared(
                    "ALTER TABLE identity_provider_configs
                     ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'",
                )
                .await?;
        }
        if !table_exists(self.db, "identity_provider_shares").await? {
            self.create_table(tables::provider_shares::statement())
                .await?;
            self.create_index(indexes::provider_shares_provider_grantee())
                .await?;
        }
        Ok(())
    }

    async fn validate_current_schema(&self) -> Result<(), DbErr> {
        if table_count(self.db, BASE_TABLES).await? != BASE_TABLES.len()
            || !provider_sharing_schema_exists(self.db).await?
            || !table_exists(self.db, "identity_activities").await?
            || table_count(self.db, MEMORY_TABLES).await? != MEMORY_TABLES.len()
        {
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

async fn required_base_tables_exist<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
    for table in BASE_TABLES {
        if table != "identity_provider_shares" && !table_exists(db, table).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn table_exists<C: ConnectionTrait>(db: &C, table: &str) -> Result<bool, DbErr> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DbBackend::Sqlite => {
            format!(
                "SELECT COUNT(*) AS table_exists FROM sqlite_master \
                 WHERE type = 'table' AND name = '{table}'"
            )
        }
        DbBackend::Postgres => format!(
            "SELECT COUNT(*) AS table_exists FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = '{table}'"
        ),
        _ => unreachable!("only SQLite and PostgreSQL are supported"),
    };
    let row = db
        .query_one_raw(Statement::from_string(backend, sql))
        .await?
        .ok_or_else(|| DbErr::Custom("database table lookup returned no row".to_owned()))?;
    row.try_get::<i64>("", "table_exists")
        .map(|count| count == 1)
}

async fn column_exists<C: ConnectionTrait>(
    db: &C,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DbBackend::Sqlite => format!(
            "SELECT COUNT(*) AS column_exists FROM pragma_table_info('{table}') WHERE name = '{column}'"
        ),
        DbBackend::Postgres => format!(
            "SELECT COUNT(*) AS column_exists FROM information_schema.columns \
             WHERE table_schema = current_schema() \
             AND table_name = '{table}' \
             AND column_name = '{column}'"
        ),
        _ => unreachable!("only SQLite and PostgreSQL are supported"),
    };
    let row = db
        .query_one_raw(Statement::from_string(backend, sql))
        .await?
        .ok_or_else(|| DbErr::Custom("database column lookup returned no row".to_owned()))?;
    row.try_get::<i64>("", "column_exists")
        .map(|count| count == 1)
}

async fn index_exists<C: ConnectionTrait>(db: &C, table: &str, index: &str) -> Result<bool, DbErr> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DbBackend::Sqlite => format!(
            "SELECT COUNT(*) AS index_exists FROM sqlite_master \
             WHERE type = 'index' AND tbl_name = '{table}' AND name = '{index}'"
        ),
        DbBackend::Postgres => format!(
            "SELECT COUNT(*) AS index_exists FROM pg_indexes \
             WHERE schemaname = current_schema() \
             AND tablename = '{table}' \
             AND indexname = '{index}'"
        ),
        _ => unreachable!("only SQLite and PostgreSQL are supported"),
    };
    let row = db
        .query_one_raw(Statement::from_string(backend, sql))
        .await?
        .ok_or_else(|| DbErr::Custom("database index lookup returned no row".to_owned()))?;
    row.try_get::<i64>("", "index_exists")
        .map(|count| count == 1)
}

async fn provider_sharing_schema_exists<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
    if !column_exists(db, "identity_provider_configs", "visibility").await? {
        return Ok(false);
    }
    for column in ["provider_id", "grantee_identity_id", "created_at"] {
        if !column_exists(db, "identity_provider_shares", column).await? {
            return Ok(false);
        }
    }
    index_exists(
        db,
        "identity_provider_shares",
        "uq_identity_provider_shares_provider_grantee",
    )
    .await
}

fn incomplete_schema_error() -> DbErr {
    DbErr::Custom(INCOMPLETE_SCHEMA_ERROR.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sea_orm::{DbBackend, MockDatabase, Value};

    use super::table_exists;

    #[tokio::test]
    async fn postgres_table_lookup_reads_the_explicit_count_alias() {
        let row = BTreeMap::from([("table_exists".to_string(), Value::BigInt(Some(0)))]);
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([[row]])
            .into_connection();

        assert!(!table_exists(&db, "identities")
            .await
            .expect("read PostgreSQL table count"));
    }
}
