use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "memories")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub normalized_key: String,
    pub conflict_key: Option<String>,
    pub kind: String,
    pub status: String,
    pub content: String,
    pub confidence: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::memory_sources::Entity")]
    Sources,
}

impl Related<super::memory_sources::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sources.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
