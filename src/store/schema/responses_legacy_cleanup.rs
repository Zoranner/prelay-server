use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, TransactionTrait};

const MIGRATION_VERSION: &str = "responses_legacy_cleanup_v1";

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

    let result = db
        .execute_unprepared(
            "DELETE FROM identity_response_sessions WHERE protocol_format = 'legacy_internal_v1'",
        )
        .await?;

    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO prelay_schema_migrations (version) VALUES ($1) ON CONFLICT (version) DO NOTHING",
        [MIGRATION_VERSION.into()],
    ))
    .await?;
    tracing::info!(
        version = MIGRATION_VERSION,
        deleted = result.rows_affected(),
        "applied schema migration"
    );
    Ok(())
}
