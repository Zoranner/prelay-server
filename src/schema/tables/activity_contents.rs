use sea_query::{ColumnDef, ForeignKey, Iden, Table, TableCreateStatement};

use super::activities::Activities;

pub(in crate::schema) fn statement() -> TableCreateStatement {
    Table::create()
        .table(ActivityContents::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(ActivityContents::Id)
                .string()
                .not_null()
                .primary_key(),
        )
        .col(
            ColumnDef::new(ActivityContents::ActivityId)
                .string()
                .not_null(),
        )
        .col(
            ColumnDef::new(ActivityContents::InputText)
                .string()
                .not_null(),
        )
        .col(
            ColumnDef::new(ActivityContents::OutputText)
                .string()
                .not_null(),
        )
        .col(ColumnDef::new(ActivityContents::MediaMetadataJson).string())
        .col(
            ColumnDef::new(ActivityContents::IsTruncated)
                .boolean()
                .not_null(),
        )
        .col(
            ColumnDef::new(ActivityContents::ContentHash)
                .string()
                .not_null(),
        )
        .col(ColumnDef::new(ActivityContents::Status).string().not_null())
        .col(
            ColumnDef::new(ActivityContents::Attempts)
                .big_integer()
                .not_null(),
        )
        .col(ColumnDef::new(ActivityContents::NextAttemptAt).string())
        .col(ColumnDef::new(ActivityContents::LeaseOwner).string())
        .col(ColumnDef::new(ActivityContents::LeaseExpiresAt).string())
        .col(ColumnDef::new(ActivityContents::LastError).string())
        .col(ColumnDef::new(ActivityContents::CompletedAt).string())
        .col(
            ColumnDef::new(ActivityContents::CreatedAt)
                .string()
                .not_null(),
        )
        .col(
            ColumnDef::new(ActivityContents::UpdatedAt)
                .string()
                .not_null(),
        )
        .foreign_key(
            ForeignKey::create()
                .from(ActivityContents::Table, ActivityContents::ActivityId)
                .to(Activities::Table, Activities::Id),
        )
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum ActivityContents {
    Table,
    Id,
    ActivityId,
    InputText,
    OutputText,
    MediaMetadataJson,
    IsTruncated,
    ContentHash,
    Status,
    Attempts,
    NextAttemptAt,
    LeaseOwner,
    LeaseExpiresAt,
    LastError,
    CompletedAt,
    CreatedAt,
    UpdatedAt,
}
