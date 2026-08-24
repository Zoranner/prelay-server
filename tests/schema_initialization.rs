use prelay_server::schema::initialize;
use sea_orm::{ConnectionTrait, Database, DbBackend, EntityTrait, Statement};

#[tokio::test]
async fn initializes_an_empty_database_without_migration_metadata() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");

    initialize(&db)
        .await
        .expect("initialize the current schema");
    prelay_server::entity::identities::Entity::find()
        .all(&db)
        .await
        .expect("identities table exists");

    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'seaql_migrations'"
                .to_owned(),
        ))
        .await
        .expect("inspect SQLite schema")
        .expect("migration table count result");
    assert_eq!(row.try_get::<i64>("", "COUNT(*)").unwrap(), 0);
}

#[tokio::test]
async fn rejects_a_partially_initialized_database() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    db.execute_unprepared("CREATE TABLE identities (id TEXT PRIMARY KEY)")
        .await
        .expect("create an incomplete schema");

    let error = initialize(&db)
        .await
        .expect_err("partial schemas require a new database deployment");
    assert!(error.to_string().contains("incomplete"));
}
