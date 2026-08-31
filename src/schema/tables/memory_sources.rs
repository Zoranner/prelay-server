use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::memories::Memories;

pub(in crate::schema) fn statement() -> TableCreateStatement {
    Table::create()
        .table(MemorySources::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(MemorySources::Id)
                .string()
                .not_null()
                .primary_key(),
        )
        .col(ColumnDef::new(MemorySources::MemoryId).string().not_null())
        .col(
            ColumnDef::new(MemorySources::IdentityId)
                .string()
                .not_null(),
        )
        .col(ColumnDef::new(MemorySources::Evidence).string().not_null())
        .col(
            ColumnDef::new(MemorySources::EvidenceHash)
                .string()
                .not_null(),
        )
        .col(
            ColumnDef::new(MemorySources::ObservedAt)
                .string()
                .not_null(),
        )
        .col(ColumnDef::new(MemorySources::CreatedAt).string().not_null())
        .foreign_key(
            ForeignKey::create()
                .from(MemorySources::Table, MemorySources::MemoryId)
                .to(Memories::Table, Memories::Id),
        )
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum MemorySources {
    Table,
    Id,
    MemoryId,
    IdentityId,
    Evidence,
    EvidenceHash,
    ObservedAt,
    CreatedAt,
}
