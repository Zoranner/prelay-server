use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, TransactionTrait};

use super::{indexes, tables};

const MIGRATION_VERSION: &str = "activity_rollout_v2";

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
    for statement in [
        "ALTER TABLE activity_rollouts ADD COLUMN IF NOT EXISTS thread_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE activity_rollouts ADD COLUMN IF NOT EXISTS window_id TEXT",
        "ALTER TABLE activity_rollouts ADD COLUMN IF NOT EXISTS generation BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE activity_rollouts ADD COLUMN IF NOT EXISTS start_line BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE activity_rollouts ADD COLUMN IF NOT EXISTS end_line BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE activity_rollouts DROP CONSTRAINT IF EXISTS uq_activity_rollouts_path",
        "DROP INDEX IF EXISTS uq_activity_rollouts_path",
    ] {
        db.execute_unprepared(statement).await?;
    }
    db.execute(&indexes::activity_rollouts_path()).await?;
    db.execute(&indexes::activity_rollouts_thread()).await?;
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
