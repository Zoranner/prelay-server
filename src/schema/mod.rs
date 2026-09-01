mod indexes;
mod tables;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement};
use sea_query::{IndexCreateStatement, TableCreateStatement};

const BASE_TABLES: [&str; 8] = [
    "identities",
    "identity_provider_configs",
    "identity_provider_models",
    "identity_endpoint_configs",
    "identity_endpoint_models",
    "identity_endpoint_model_routes",
    "identity_response_sessions",
    "identity_model_aliases",
];
const LEGACY_ACTIVITY_TABLE: &str = "identity_request_logs";
const LEGACY_ACTIVITY_INDEX: &str = "idx_identity_request_logs_identity_created_at";
const MEMORY_TABLES: [&str; 3] = ["activity_contents", "memories", "memory_sources"];

pub async fn initialize(db: &DatabaseConnection) -> Result<(), DbErr> {
    let manager = SchemaInitializer { db };
    let existing_base_tables = table_count(db, BASE_TABLES).await?;
    let has_current_activities = table_exists(db, "identity_activities").await?;
    let has_legacy_activities = table_exists(db, LEGACY_ACTIVITY_TABLE).await?;
    let existing_memory_tables = table_count(db, MEMORY_TABLES).await?;

    if existing_base_tables == 0
        && !has_current_activities
        && !has_legacy_activities
        && existing_memory_tables == 0
    {
        initialize_empty_schema(&manager).await?;
        return Ok(());
    }
    if existing_base_tables != BASE_TABLES.len() {
        return Err(DbErr::Custom(
            "database schema is incomplete; create a new database deployment".to_owned(),
        ));
    }

    if !has_current_activities && has_legacy_activities {
        manager.rename_legacy_activities().await?;
    }
    if !has_current_activities && !has_legacy_activities {
        return Err(DbErr::Custom(
            "database schema is incomplete; create a new database deployment".to_owned(),
        ));
    }
    manager.drop_legacy_cost_columns().await?;
    match existing_memory_tables {
        0 => initialize_memory_schema(&manager).await,
        count if count == MEMORY_TABLES.len() => Ok(()),
        _ => Err(DbErr::Custom(
            "database schema is incomplete; create a new database deployment".to_owned(),
        )),
    }
}

async fn initialize_empty_schema(manager: &SchemaInitializer<'_>) -> Result<(), DbErr> {
    manager.create_table(tables::identity::statement()).await?;
    manager.create_table(tables::providers::configs()).await?;
    manager.create_table(tables::providers::models()).await?;
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

async fn initialize_memory_schema(manager: &SchemaInitializer<'_>) -> Result<(), DbErr> {
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

async fn table_count<'a>(
    db: &DatabaseConnection,
    tables: impl IntoIterator<Item = &'a str>,
) -> Result<usize, DbErr> {
    let mut count = 0;
    for table in tables {
        count += usize::from(table_exists(db, table).await?);
    }
    Ok(count)
}

struct SchemaInitializer<'a> {
    db: &'a DatabaseConnection,
}

impl SchemaInitializer<'_> {
    async fn create_table(&self, statement: TableCreateStatement) -> Result<(), DbErr> {
        self.db.execute(&statement).await.map(|_| ())
    }

    async fn create_index(&self, statement: IndexCreateStatement) -> Result<(), DbErr> {
        self.db.execute(&statement).await.map(|_| ())
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

async fn table_exists(db: &DatabaseConnection, table: &str) -> Result<bool, DbErr> {
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

async fn column_exists(db: &DatabaseConnection, table: &str, column: &str) -> Result<bool, DbErr> {
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
