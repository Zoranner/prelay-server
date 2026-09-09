use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::identity::Identities;

pub(in crate::schema) fn configs() -> TableCreateStatement {
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
        .col(
            ColumnDef::new(ProviderConfigs::Visibility)
                .string()
                .not_null()
                .default("private"),
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
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum ProviderConfigs {
    #[iden = "identity_provider_configs"]
    Table,
    Id,
    IdentityId,
    Name,
    ProviderType,
    Visibility,
    BaseUrl,
    ApiKeyCiphertext,
    CapabilitiesJson,
    CreatedAt,
}
