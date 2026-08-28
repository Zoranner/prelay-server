use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::{super::indexes, identity::Identities, providers::ProviderConfigs};

pub(in crate::schema) fn configs() -> TableCreateStatement {
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
        .to_owned()
}

pub(in crate::schema) fn models() -> TableCreateStatement {
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
        .index(&mut indexes::endpoint_models_candidate())
        .to_owned()
}

pub(in crate::schema) fn routes() -> TableCreateStatement {
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
        .primary_key(&mut indexes::endpoint_routes_primary_key())
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
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum EndpointConfigs {
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
pub(in crate::schema) enum EndpointModels {
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
pub(in crate::schema) enum EndpointRoutes {
    #[iden = "identity_endpoint_model_routes"]
    Table,
    EndpointId,
    ModelName,
    ProviderId,
    UpdatedAt,
}
