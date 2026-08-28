use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_endpoint_configs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub identity_id: String,
    pub name: String,
    pub protocol: String,
    pub token: String,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::super::identities::Entity",
        from = "Column::IdentityId",
        to = "super::super::identities::Column::Id"
    )]
    Identity,
    #[sea_orm(has_many = "super::endpoint_models::Entity")]
    EndpointModels,
    #[sea_orm(has_many = "super::endpoint_model_routes::Entity")]
    EndpointModelRoutes,
}

impl Related<super::super::identities::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Identity.def()
    }
}

impl Related<super::endpoint_models::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EndpointModels.def()
    }
}

impl Related<super::endpoint_model_routes::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EndpointModelRoutes.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
