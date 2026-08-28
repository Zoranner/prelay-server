use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::identity::Identities;

pub(in crate::schema) fn statement() -> TableCreateStatement {
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
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum RequestLogs {
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
