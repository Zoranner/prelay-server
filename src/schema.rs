use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement};
use sea_query::{
    ColumnDef, ForeignKey, Iden, Index, IndexCreateStatement, Table, TableCreateStatement,
};

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

    manager
        .create_table(
            Table::create()
                .table(Identities::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Identities::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(Identities::MachineId).string().not_null())
                .col(ColumnDef::new(Identities::AccountSid).string().not_null())
                .col(
                    ColumnDef::new(Identities::CredentialHash)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(Identities::DisplayName)
                        .string()
                        .not_null()
                        .default(""),
                )
                .col(ColumnDef::new(Identities::CreatedAt).string().not_null())
                .col(ColumnDef::new(Identities::LastActiveAt).string().not_null())
                .index(
                    Index::create()
                        .name("uq_identities_machine_sid")
                        .col(Identities::MachineId)
                        .col(Identities::AccountSid)
                        .unique(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(ProviderConfigs::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(ProviderConfigs::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(ProviderConfigs::IdentityId)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(ProviderConfigs::Name).string().not_null())
                .col(
                    ColumnDef::new(ProviderConfigs::ProviderType)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(ProviderConfigs::BaseUrl).string().not_null())
                .col(
                    ColumnDef::new(ProviderConfigs::ApiKeyCiphertext)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(ProviderConfigs::CapabilitiesJson).string())
                .col(
                    ColumnDef::new(ProviderConfigs::CreatedAt)
                        .string()
                        .not_null(),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(ProviderConfigs::Table, ProviderConfigs::IdentityId)
                        .to(Identities::Table, Identities::Id),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(ProviderModels::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(ProviderModels::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(ProviderModels::ProviderId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(ProviderModels::ModelName)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(ProviderModels::CreatedAt)
                        .string()
                        .not_null(),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(ProviderModels::Table, ProviderModels::ProviderId)
                        .to(ProviderConfigs::Table, ProviderConfigs::Id),
                )
                .index(
                    Index::create()
                        .name("uq_provider_models_name")
                        .col(ProviderModels::ProviderId)
                        .col(ProviderModels::ModelName)
                        .unique(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(EndpointConfigs::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(EndpointConfigs::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(EndpointConfigs::IdentityId)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(EndpointConfigs::Name).string().not_null())
                .col(
                    ColumnDef::new(EndpointConfigs::Protocol)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(EndpointConfigs::Token)
                        .string()
                        .not_null()
                        .unique_key(),
                )
                .col(
                    ColumnDef::new(EndpointConfigs::CreatedAt)
                        .string()
                        .not_null(),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(EndpointConfigs::Table, EndpointConfigs::IdentityId)
                        .to(Identities::Table, Identities::Id),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(EndpointModels::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(EndpointModels::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(EndpointModels::EndpointId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(EndpointModels::ModelName)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(EndpointModels::ProviderId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(EndpointModels::UpstreamModel)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(EndpointModels::CandidateOrder)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    ColumnDef::new(EndpointModels::CreatedAt)
                        .string()
                        .not_null(),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(EndpointModels::Table, EndpointModels::EndpointId)
                        .to(EndpointConfigs::Table, EndpointConfigs::Id),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(EndpointModels::Table, EndpointModels::ProviderId)
                        .to(ProviderConfigs::Table, ProviderConfigs::Id),
                )
                .index(
                    Index::create()
                        .name("uq_endpoint_models_candidate")
                        .col(EndpointModels::EndpointId)
                        .col(EndpointModels::ModelName)
                        .col(EndpointModels::ProviderId)
                        .col(EndpointModels::UpstreamModel)
                        .unique(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(EndpointRoutes::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(EndpointRoutes::EndpointId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(EndpointRoutes::ModelName)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(EndpointRoutes::ProviderId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(EndpointRoutes::UpdatedAt)
                        .string()
                        .not_null(),
                )
                .primary_key(
                    Index::create()
                        .col(EndpointRoutes::EndpointId)
                        .col(EndpointRoutes::ModelName),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(EndpointRoutes::Table, EndpointRoutes::EndpointId)
                        .to(EndpointConfigs::Table, EndpointConfigs::Id),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(EndpointRoutes::Table, EndpointRoutes::ProviderId)
                        .to(ProviderConfigs::Table, ProviderConfigs::Id),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(ResponseSessions::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(ResponseSessions::ResponseId)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(ResponseSessions::IdentityId)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(ResponseSessions::PreviousResponseId).string())
                .col(
                    ColumnDef::new(ResponseSessions::ProviderId)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(ResponseSessions::Model).string().not_null())
                .col(
                    ColumnDef::new(ResponseSessions::InputMessagesJson)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(ResponseSessions::OutputItemsJson)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(ResponseSessions::CreatedAt)
                        .string()
                        .not_null(),
                )
                .primary_key(
                    Index::create()
                        .col(ResponseSessions::ResponseId)
                        .col(ResponseSessions::IdentityId),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(ResponseSessions::Table, ResponseSessions::IdentityId)
                        .to(Identities::Table, Identities::Id),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(ResponseSessions::Table, ResponseSessions::ProviderId)
                        .to(ProviderConfigs::Table, ProviderConfigs::Id),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(RequestLogs::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(RequestLogs::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(RequestLogs::IdentityId).string().not_null())
                .col(ColumnDef::new(RequestLogs::CreatedAt).string().not_null())
                .col(ColumnDef::new(RequestLogs::ProtocolIn).string())
                .col(ColumnDef::new(RequestLogs::ProtocolOut).string())
                .col(ColumnDef::new(RequestLogs::ProtocolUpstream).string())
                .col(ColumnDef::new(RequestLogs::EndpointName).string())
                .col(ColumnDef::new(RequestLogs::ProviderId).string())
                .col(ColumnDef::new(RequestLogs::ProviderName).string())
                .col(ColumnDef::new(RequestLogs::ModelRequested).string())
                .col(ColumnDef::new(RequestLogs::ModelUpstream).string())
                .col(ColumnDef::new(RequestLogs::ProxyTokenId).string())
                .col(ColumnDef::new(RequestLogs::Status).string().not_null())
                .col(ColumnDef::new(RequestLogs::HttpStatus).big_integer())
                .col(ColumnDef::new(RequestLogs::ErrorCode).string())
                .col(ColumnDef::new(RequestLogs::ErrorMessage).string())
                .col(ColumnDef::new(RequestLogs::IsStreaming).boolean())
                .col(ColumnDef::new(RequestLogs::InputTokens).big_integer())
                .col(ColumnDef::new(RequestLogs::OutputTokens).big_integer())
                .col(ColumnDef::new(RequestLogs::ReasoningTokens).big_integer())
                .col(ColumnDef::new(RequestLogs::CacheReadTokens).big_integer())
                .col(ColumnDef::new(RequestLogs::CacheWriteTokens).big_integer())
                .col(ColumnDef::new(RequestLogs::EstimatedCost).double())
                .col(ColumnDef::new(RequestLogs::Currency).string())
                .col(ColumnDef::new(RequestLogs::LatencyMs).big_integer())
                .col(ColumnDef::new(RequestLogs::UpstreamLatencyMs).big_integer())
                .col(ColumnDef::new(RequestLogs::FirstTokenMs).big_integer())
                .col(ColumnDef::new(RequestLogs::ToolCallCount).big_integer())
                .col(ColumnDef::new(RequestLogs::UpstreamRequestId).string())
                .col(ColumnDef::new(RequestLogs::MetadataJson).string())
                .foreign_key(
                    ForeignKey::create()
                        .from(RequestLogs::Table, RequestLogs::IdentityId)
                        .to(Identities::Table, Identities::Id),
                )
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_identity_request_logs_identity_created_at")
                .table(RequestLogs::Table)
                .col(RequestLogs::IdentityId)
                .col(RequestLogs::CreatedAt)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(ModelAliases::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(ModelAliases::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(ModelAliases::IdentityId).string().not_null())
                .col(ColumnDef::new(ModelAliases::Alias).string().not_null())
                .col(ColumnDef::new(ModelAliases::ProviderId).string().not_null())
                .col(
                    ColumnDef::new(ModelAliases::UpstreamModel)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(ModelAliases::DownstreamProtocolsJson)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(ModelAliases::Enabled)
                        .boolean()
                        .not_null()
                        .default(true),
                )
                .col(ColumnDef::new(ModelAliases::CreatedAt).string().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .from(ModelAliases::Table, ModelAliases::IdentityId)
                        .to(Identities::Table, Identities::Id),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(ModelAliases::Table, ModelAliases::ProviderId)
                        .to(ProviderConfigs::Table, ProviderConfigs::Id),
                )
                .index(
                    Index::create()
                        .name("uq_identity_model_aliases_alias")
                        .col(ModelAliases::IdentityId)
                        .col(ModelAliases::Alias)
                        .unique(),
                )
                .to_owned(),
        )
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

#[derive(Iden)]
enum Identities {
    Table,
    Id,
    MachineId,
    AccountSid,
    CredentialHash,
    DisplayName,
    CreatedAt,
    LastActiveAt,
}

#[derive(Iden)]
enum ProviderConfigs {
    #[iden = "identity_provider_configs"]
    Table,
    Id,
    IdentityId,
    Name,
    ProviderType,
    BaseUrl,
    ApiKeyCiphertext,
    CapabilitiesJson,
    CreatedAt,
}

#[derive(Iden)]
enum ProviderModels {
    #[iden = "identity_provider_models"]
    Table,
    Id,
    ProviderId,
    ModelName,
    CreatedAt,
}

#[derive(Iden)]
enum EndpointConfigs {
    #[iden = "identity_endpoint_configs"]
    Table,
    Id,
    IdentityId,
    Name,
    Protocol,
    Token,
    CreatedAt,
}

#[derive(Iden)]
enum EndpointModels {
    #[iden = "identity_endpoint_models"]
    Table,
    Id,
    EndpointId,
    ModelName,
    ProviderId,
    UpstreamModel,
    CandidateOrder,
    CreatedAt,
}

#[derive(Iden)]
enum EndpointRoutes {
    #[iden = "identity_endpoint_model_routes"]
    Table,
    EndpointId,
    ModelName,
    ProviderId,
    UpdatedAt,
}

#[derive(Iden)]
enum ResponseSessions {
    #[iden = "identity_response_sessions"]
    Table,
    ResponseId,
    IdentityId,
    PreviousResponseId,
    ProviderId,
    Model,
    InputMessagesJson,
    OutputItemsJson,
    CreatedAt,
}

#[derive(Iden)]
enum RequestLogs {
    #[iden = "identity_request_logs"]
    Table,
    Id,
    IdentityId,
    CreatedAt,
    ProtocolIn,
    ProtocolOut,
    ProtocolUpstream,
    EndpointName,
    ProviderId,
    ProviderName,
    ModelRequested,
    ModelUpstream,
    ProxyTokenId,
    Status,
    HttpStatus,
    ErrorCode,
    ErrorMessage,
    IsStreaming,
    InputTokens,
    OutputTokens,
    ReasoningTokens,
    CacheReadTokens,
    CacheWriteTokens,
    EstimatedCost,
    Currency,
    LatencyMs,
    UpstreamLatencyMs,
    FirstTokenMs,
    ToolCallCount,
    UpstreamRequestId,
    MetadataJson,
}

#[derive(Iden)]
enum ModelAliases {
    #[iden = "identity_model_aliases"]
    Table,
    Id,
    IdentityId,
    Alias,
    ProviderId,
    UpstreamModel,
    DownstreamProtocolsJson,
    Enabled,
    CreatedAt,
}
