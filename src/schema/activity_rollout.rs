use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, TransactionTrait};

use super::{indexes, tables};

const MIGRATION_VERSION: &str = "activity_rollout_v1";

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
    db.execute(&tables::activity_rollouts::statement()).await?;
    db.execute(&indexes::activity_rollouts_path()).await?;
    db.execute(&indexes::activity_rollouts_identity_created_at())
        .await?;
    db.execute(&indexes::activity_rollouts_session()).await?;
    db.execute_unprepared("DROP TABLE IF EXISTS activity_contents")
        .await?;
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO prelay_schema_migrations (version) VALUES ($1) ON CONFLICT (version) DO NOTHING",
        [MIGRATION_VERSION.into()],
    ))
    .await?;
    Ok(())
}
