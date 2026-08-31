use sea_query::{ColumnDef, Iden, Table, TableCreateStatement};

pub(in crate::schema) fn statement() -> TableCreateStatement {
    Table::create()
        .table(Memories::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(Memories::Id)
                .string()
                .not_null()
                .primary_key(),
        )
        .col(ColumnDef::new(Memories::NormalizedKey).string().not_null())
        .col(ColumnDef::new(Memories::ConflictKey).string())
        .col(ColumnDef::new(Memories::Kind).string().not_null())
        .col(ColumnDef::new(Memories::Status).string().not_null())
        .col(ColumnDef::new(Memories::Content).string().not_null())
        .col(ColumnDef::new(Memories::Confidence).double().not_null())
        .col(ColumnDef::new(Memories::CreatedAt).string().not_null())
        .col(ColumnDef::new(Memories::UpdatedAt).string().not_null())
        .to_owned()
}

#[derive(Iden)]
pub(in crate::schema) enum Memories {
    Table,
    Id,
    NormalizedKey,
    ConflictKey,
    Kind,
    Status,
    Content,
    Confidence,
    CreatedAt,
    UpdatedAt,
}
