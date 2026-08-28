use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::{super::indexes, identity::Identities, providers::ProviderConfigs};

pub(in crate::schema) fn statement() -> TableCreateStatement {
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
        .primary_key(&mut indexes::response_sessions_primary_key())
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
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum ResponseSessions {
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
