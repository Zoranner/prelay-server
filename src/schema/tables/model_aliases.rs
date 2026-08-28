use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::{super::indexes, identity::Identities, providers::ProviderConfigs};

pub(in crate::schema) fn statement() -> TableCreateStatement {
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
        .index(&mut indexes::model_aliases_alias())
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum ModelAliases {
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
