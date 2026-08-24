use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_endpoint_models")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub endpoint_id: String,
    pub model_name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub candidate_order: i64,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::identity_endpoint_configs::Entity",
        from = "Column::EndpointId",
        to = "super::identity_endpoint_configs::Column::Id"
    )]
    Endpoint,
    #[sea_orm(
        belongs_to = "super::identity_provider_configs::Entity",
        from = "Column::ProviderId",
        to = "super::identity_provider_configs::Column::Id"
    )]
    Provider,
}

impl Related<super::identity_endpoint_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Endpoint.def()
    }
}

impl Related<super::identity_provider_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Provider.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
