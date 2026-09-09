use sea_query::{
    ColumnDef, ForeignKey, ForeignKeyAction, Iden, Index, Table, TableCreateStatement,
};

use super::{identity::Identities, providers::ProviderConfigs};

pub(in crate::schema) fn statement() -> TableCreateStatement {
    Table::create()
        .table(ProviderShares::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(ProviderShares::ProviderId)
                .string()
                .not_null(),
        )
        .col(
            ColumnDef::new(ProviderShares::GranteeIdentityId)
                .string()
                .not_null(),
        )
        .col(
            ColumnDef::new(ProviderShares::CreatedAt)
                .string()
                .not_null(),
        )
        .primary_key(
            &mut Index::create()
                .col(ProviderShares::ProviderId)
                .col(ProviderShares::GranteeIdentityId)
                .to_owned(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(ProviderShares::Table, ProviderShares::ProviderId)
                .to(ProviderConfigs::Table, ProviderConfigs::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .foreign_key(
            ForeignKey::create()
                .from(ProviderShares::Table, ProviderShares::GranteeIdentityId)
                .to(Identities::Table, Identities::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum ProviderShares {
    #[iden = "identity_provider_shares"]
    Table,
    ProviderId,
    GranteeIdentityId,
    CreatedAt,
}
