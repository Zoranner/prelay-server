use prelay_server::{store::schema::initialize, test_support::test_empty_database_connection};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

async fn column_exists(db: &DatabaseConnection, table: &str, column: &str) -> bool {
    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "SELECT COUNT(*) AS result_count FROM information_schema.columns \
                 WHERE table_schema = current_schema() \
                 AND table_name = '{table}' \
                 AND column_name = '{column}'"
            ),
        ))
        .await
        .expect("inspect schema columns")
        .expect("column count result");
    row.try_get::<i64>("", "result_count").unwrap() == 1
}

async fn index_exists(db: &DatabaseConnection, table: &str, index: &str) -> bool {
    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "SELECT COUNT(*) AS result_count FROM pg_indexes \
                 WHERE schemaname = current_schema() \
                 AND tablename = '{table}' AND indexname = '{index}'"
            ),
        ))
        .await
        .expect("inspect schema indexes")
        .expect("index count result");
    row.try_get::<i64>("", "result_count").unwrap() == 1
}

#[tokio::test]
async fn upgrades_activity_rollouts_to_shared_thread_files() {
    let db = test_empty_database_connection().await;
    initialize(&db).await.expect("initialize current schema");

    db.execute_unprepared(
        "DROP INDEX idx_activity_rollouts_identity_thread;
         DROP INDEX idx_activity_rollouts_path;
         ALTER TABLE activity_rollouts
             DROP COLUMN thread_id,
             DROP COLUMN window_id,
             DROP COLUMN generation,
             DROP COLUMN start_line,
             DROP COLUMN end_line;
         CREATE UNIQUE INDEX uq_activity_rollouts_path
             ON activity_rollouts (rollout_path)",
    )
    .await
    .expect("simulate the previous rollout schema");

    initialize(&db)
        .await
        .expect("upgrade the previous rollout schema");

    for column in [
        "thread_id",
        "window_id",
        "generation",
        "start_line",
        "end_line",
    ] {
        assert!(
            column_exists(&db, "activity_rollouts", column).await,
            "missing upgraded column activity_rollouts.{column}"
        );
    }
    assert!(
        index_exists(
            &db,
            "activity_rollouts",
            "idx_activity_rollouts_identity_thread"
        )
        .await
    );
    assert!(
        !index_exists(&db, "activity_rollouts", "uq_activity_rollouts_path").await,
        "the old unique rollout path index must be removed"
    );
}
