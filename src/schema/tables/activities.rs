use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::identity::Identities;

pub(in crate::schema) fn statement() -> TableCreateStatement {
    Table::create()
        .table(Activities::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(Activities::Id)
                .string()
                .not_null()
                .primary_key(),
        )
        .col(ColumnDef::new(Activities::IdentityId).string().not_null())
        .col(ColumnDef::new(Activities::CreatedAt).string().not_null())
        .col(ColumnDef::new(Activities::ProtocolIn).string())
        .col(ColumnDef::new(Activities::ProtocolOut).string())
        .col(ColumnDef::new(Activities::ProtocolUpstream).string())
        .col(ColumnDef::new(Activities::EndpointName).string())
        .col(ColumnDef::new(Activities::ProviderId).string())
        .col(ColumnDef::new(Activities::ProviderName).string())
        .col(ColumnDef::new(Activities::ModelRequested).string())
        .col(ColumnDef::new(Activities::ModelUpstream).string())
        .col(ColumnDef::new(Activities::ProxyTokenId).string())
        .col(ColumnDef::new(Activities::Status).string().not_null())
        .col(ColumnDef::new(Activities::HttpStatus).big_integer())
        .col(ColumnDef::new(Activities::ErrorCode).string())
        .col(ColumnDef::new(Activities::ErrorMessage).string())
        .col(ColumnDef::new(Activities::IsStreaming).boolean())
        .col(ColumnDef::new(Activities::InputTokens).big_integer())
        .col(ColumnDef::new(Activities::OutputTokens).big_integer())
        .col(ColumnDef::new(Activities::ReasoningTokens).big_integer())
        .col(ColumnDef::new(Activities::CacheReadTokens).big_integer())
        .col(ColumnDef::new(Activities::CacheWriteTokens).big_integer())
        .col(ColumnDef::new(Activities::LatencyMs).big_integer())
        .col(ColumnDef::new(Activities::UpstreamLatencyMs).big_integer())
        .col(ColumnDef::new(Activities::FirstTokenMs).big_integer())
        .col(ColumnDef::new(Activities::ToolCallCount).big_integer())
        .col(ColumnDef::new(Activities::UpstreamRequestId).string())
        .col(ColumnDef::new(Activities::MetadataJson).string())
        .foreign_key(
            ForeignKey::create()
                .from(Activities::Table, Activities::IdentityId)
                .to(Identities::Table, Identities::Id),
        )
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum Activities {
    #[iden = "identity_activities"]
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
    LatencyMs,
    UpstreamLatencyMs,
    FirstTokenMs,
    ToolCallCount,
    UpstreamRequestId,
    MetadataJson,
}
