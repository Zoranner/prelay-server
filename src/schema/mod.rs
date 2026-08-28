mod indexes;
mod tables;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement};
use sea_query::{IndexCreateStatement, TableCreateStatement};

const TABLES: [&str; 9] = [
    "identities",
    "identity_provider_configs",
    "identity_provider_models",
    "identity_endpoint_configs",
    "identity_endpoint_models",
    "identity_endpoint_model_routes",
    "identity_response_sessions",
    "identity_request_logs",
    "identity_model_aliases",
];

pub async fn initialize(db: &DatabaseConnection) -> Result<(), DbErr> {
    let manager = SchemaInitializer { db };
    let mut existing_tables = 0;
    for table in TABLES {
        existing_tables += usize::from(table_exists(db, table).await?);
    }
    if existing_tables == TABLES.len() {
        return Ok(());
    }
    if existing_tables > 0 {
        return Err(DbErr::Custom(
            "database schema is incomplete; create a new database deployment".to_owned(),
        ));
    }

    manager.create_table(tables::identity::statement()).await?;
    manager.create_table(tables::providers::configs()).await?;
    manager.create_table(tables::providers::models()).await?;
    manager.create_table(tables::endpoints::configs()).await?;
    manager.create_table(tables::endpoints::models()).await?;
    manager.create_table(tables::endpoints::routes()).await?;
    manager.create_table(tables::sessions::statement()).await?;
    manager
        .create_table(tables::request_logs::statement())
        .await?;
    manager
        .create_index(indexes::request_logs_identity_created_at())
        .await?;
    manager
        .create_table(tables::model_aliases::statement())
        .await
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
