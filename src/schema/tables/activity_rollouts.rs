use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::activities::Activities;

pub(in crate::schema) fn statement() -> TableCreateStatement {
    Table::create()
        .table(ActivityRollouts::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(ActivityRollouts::ActivityId)
                .string()
                .not_null()
                .primary_key(),
        )
        .col(
            ColumnDef::new(ActivityRollouts::IdentityId)
                .string()
                .not_null(),
        )
        .col(
            ColumnDef::new(ActivityRollouts::SessionId)
                .string()
                .not_null(),
        )
        .col(
            ColumnDef::new(ActivityRollouts::RolloutPath)
                .string()
                .not_null(),
        )
        .col(ColumnDef::new(ActivityRollouts::Status).string().not_null())
        .col(
            ColumnDef::new(ActivityRollouts::EventCount)
                .big_integer()
                .not_null()
                .default(0),
        )
        .col(
            ColumnDef::new(ActivityRollouts::ByteSize)
                .big_integer()
                .not_null()
                .default(0),
        )
        .col(ColumnDef::new(ActivityRollouts::Sha256).string())
        .col(
            ColumnDef::new(ActivityRollouts::CreatedAt)
                .string()
                .not_null(),
        )
        .col(ColumnDef::new(ActivityRollouts::CompletedAt).string())
        .col(ColumnDef::new(ActivityRollouts::LastError).string())
        .foreign_key(
            ForeignKey::create()
                .from(ActivityRollouts::Table, ActivityRollouts::ActivityId)
                .to(Activities::Table, Activities::Id)
                .on_delete(sea_query::ForeignKeyAction::Cascade),
        )
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum ActivityRollouts {
    Table,
    ActivityId,
    IdentityId,
    SessionId,
    RolloutPath,
    Status,
    EventCount,
    ByteSize,
    Sha256,
    CreatedAt,
    CompletedAt,
    LastError,
}
