use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_endpoint_model_routes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub endpoint_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub model_name: String,
    pub provider_id: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::endpoint_configs::Entity",
        from = "Column::EndpointId",
        to = "super::endpoint_configs::Column::Id"
    )]
    Endpoint,
    #[sea_orm(
        belongs_to = "super::provider_configs::Entity",
        from = "Column::ProviderId",
        to = "super::provider_configs::Column::Id"
    )]
    Provider,
}

impl Related<super::endpoint_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Endpoint.def()
    }
}

impl Related<super::provider_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Provider.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
