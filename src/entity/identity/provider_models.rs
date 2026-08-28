use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_provider_models")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub provider_id: String,
    pub model_name: String,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::provider_configs::Entity",
        from = "Column::ProviderId",
        to = "super::provider_configs::Column::Id"
    )]
    Provider,
}

impl Related<super::provider_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Provider.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
