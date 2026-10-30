use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "memory_sources")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub memory_id: String,
    pub identity_id: String,
    pub evidence: String,
    pub evidence_hash: String,
    pub observed_at: String,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::memories::Entity",
        from = "Column::MemoryId",
        to = "super::memories::Column::Id"
    )]
    Memory,
}

impl Related<super::memories::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Memory.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
