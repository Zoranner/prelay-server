use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement, TransactionTrait};

const MIGRATION_VERSION: &str = "activity_content_lifecycle_v1";

pub(super) async fn apply(db: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let transaction = db.begin().await?;
    transaction
        .execute_unprepared(
            "CREATE TABLE IF NOT EXISTS prelay_schema_migrations (
                version VARCHAR(128) PRIMARY KEY,
                applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .await?;

    let claim = transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            match backend {
                DbBackend::Postgres => {
                    "INSERT INTO prelay_schema_migrations (version) VALUES ($1) ON CONFLICT (version) DO NOTHING"
                }
                _ => unreachable!("only PostgreSQL is supported"),
            },
            [MIGRATION_VERSION.into()],
        ))
        .await?;
    if claim.rows_affected() == 0 {
        transaction.commit().await?;
        return Ok(());
    }

    let update_sql = match backend {
        DbBackend::Postgres => {
            "UPDATE activity_contents AS c
             SET status = 'pending',
                 next_attempt_at = CURRENT_TIMESTAMP::text,
                 lease_owner = NULL,
                 lease_expires_at = NULL,
                 updated_at = CURRENT_TIMESTAMP::text
             WHERE c.status = 'capturing'
               AND c.updated_at::timestamptz < CURRENT_TIMESTAMP - INTERVAL '5 minutes'
               AND EXISTS (
                   SELECT 1
                   FROM identity_activities AS a
                   WHERE a.id = c.activity_id
                     AND a.status IN ('success', 'failed')
               )"
        }
        _ => unreachable!("only PostgreSQL is supported"),
    };
    transaction.execute_unprepared(update_sql).await?;
    transaction.commit().await
}
