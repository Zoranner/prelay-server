use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::{super::indexes, identity::Identities};

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

pub(in crate::schema) fn models() -> TableCreateStatement {
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
        .index(&mut indexes::provider_models_name())
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
    BaseUrl,
    ApiKeyCiphertext,
    CapabilitiesJson,
    CreatedAt,
}

#[derive(Iden)]
pub(in crate::schema) enum ProviderModels {
    #[iden = "identity_provider_models"]
    Table,
    Id,
    ProviderId,
    ModelName,
    CreatedAt,
}
