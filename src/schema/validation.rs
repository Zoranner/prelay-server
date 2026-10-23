use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};

use super::{BASE_TABLES, INCOMPLETE_SCHEMA_ERROR, MEMORY_TABLES};

pub(super) async fn table_count<'a, C: ConnectionTrait>(
    db: &C,
    tables: impl IntoIterator<Item = &'a str>,
) -> Result<usize, DbErr> {
    let mut count = 0;
    for table in tables {
        count += usize::from(table_exists(db, table).await?);
    }
    Ok(count)
}

pub(super) async fn activity_rollout_schema_exists<C: ConnectionTrait>(
    db: &C,
) -> Result<bool, DbErr> {
    for column in [
        "activity_id",
        "identity_id",
        "session_id",
        "rollout_path",
        "status",
        "event_count",
        "byte_size",
        "sha256",
        "created_at",
        "completed_at",
        "last_error",
    ] {
        if !column_exists(db, "activity_rollouts", column).await? {
            return Ok(false);
        }
    }
    for index in [
        "uq_activity_rollouts_path",
        "idx_activity_rollouts_identity_created_at",
        "idx_activity_rollouts_session",
    ] {
        if !index_exists(db, "activity_rollouts", index).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) async fn response_session_schema_exists<C: ConnectionTrait>(
    db: &C,
) -> Result<bool, DbErr> {
    for column in [
        "response_id",
        "identity_id",
        "previous_response_id",
        "conversation_id",
        "provider_id",
        "model",
        "input_messages_json",
        "output_items_json",
        "created_at",
    ] {
        if !column_exists(db, "identity_response_sessions", column).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) async fn required_base_tables_exist<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
    for table in BASE_TABLES {
        if table != "identity_provider_shares" && !table_exists(db, table).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) async fn table_exists<C: ConnectionTrait>(db: &C, table: &str) -> Result<bool, DbErr> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => format!(
            "SELECT COUNT(*) AS table_exists FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = '{table}'"
        ),
        _ => unreachable!("only PostgreSQL is supported"),
    };
    let row = db
        .query_one_raw(Statement::from_string(backend, sql))
        .await?
        .ok_or_else(|| DbErr::Custom("database table lookup returned no row".to_owned()))?;
    row.try_get::<i64>("", "table_exists")
        .map(|count| count == 1)
}

pub(crate) async fn column_exists<C: ConnectionTrait>(
    db: &C,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let backend = db.get_database_backend();
    let sql = match backend {
        DbBackend::Postgres => format!(
            "SELECT COUNT(*) AS column_exists FROM information_schema.columns \
             WHERE table_schema = current_schema() \
             AND table_name = '{table}' \
             AND column_name = '{column}'"
        ),
        _ => unreachable!("only PostgreSQL is supported"),
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
        DbBackend::Postgres => format!(
            "SELECT COUNT(*) AS index_exists FROM pg_indexes \
             WHERE schemaname = current_schema() \
             AND tablename = '{table}' \
             AND indexname = '{index}'"
        ),
        _ => unreachable!("only PostgreSQL is supported"),
    };
    let row = db
        .query_one_raw(Statement::from_string(backend, sql))
        .await?
        .ok_or_else(|| DbErr::Custom("database index lookup returned no row".to_owned()))?;
    row.try_get::<i64>("", "index_exists")
        .map(|count| count == 1)
}

pub(super) async fn provider_sharing_schema_exists<C: ConnectionTrait>(
    db: &C,
) -> Result<bool, DbErr> {
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

pub(super) fn incomplete_schema_error() -> DbErr {
    DbErr::Custom(INCOMPLETE_SCHEMA_ERROR.to_owned())
}

pub(super) async fn current_schema_is_complete<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
    Ok(table_count(db, BASE_TABLES).await? == BASE_TABLES.len()
        && provider_sharing_schema_exists(db).await?
        && table_exists(db, "identity_activities").await?
        && table_count(db, MEMORY_TABLES).await? == MEMORY_TABLES.len()
        && activity_rollout_schema_exists(db).await?
        && response_session_schema_exists(db).await?)
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
