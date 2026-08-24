mod m20260823_000001_initial_schema;

use sea_orm::DatabaseConnection;
use sea_orm_migration::{prelude::*, MigratorTrait};

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260823_000001_initial_schema::Migration)]
    }
}

#[derive(Debug)]
pub enum MigrationError {
    Database(DbErr),
    SchemaOutdated { pending: usize },
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(_) => formatter.write_str("database migration check failed"),
            Self::SchemaOutdated { pending } => write!(
                formatter,
                "database schema has {pending} pending migrations"
            ),
        }
    }
}

impl std::error::Error for MigrationError {}

pub async fn apply_all(db: &DatabaseConnection) -> Result<(), MigrationError> {
    Migrator::up(db, None)
        .await
        .map_err(MigrationError::Database)
}

pub async fn ensure_current(db: &DatabaseConnection) -> Result<(), MigrationError> {
    let pending = Migrator::get_pending_migrations_read_only(db)
        .await
        .map_err(MigrationError::Database)?;
    if pending.is_empty() {
        Ok(())
    } else {
        Err(MigrationError::SchemaOutdated {
            pending: pending.len(),
        })
    }
}
