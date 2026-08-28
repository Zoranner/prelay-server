use sea_query::{ColumnDef, Iden, Table, TableCreateStatement};

use super::super::indexes;

pub(in crate::schema) fn statement() -> TableCreateStatement {
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
        .index(&mut indexes::identities_machine_sid())
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum Identities {
    Table,
    Id,
    MachineId,
    AccountSid,
    CredentialHash,
    DisplayName,
    CreatedAt,
    LastActiveAt,
}
