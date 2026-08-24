use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_provider_configs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub identity_id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key_ciphertext: String,
    pub capabilities_json: Option<String>,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::identities::Entity",
        from = "Column::IdentityId",
        to = "super::identities::Column::Id"
    )]
    Identity,
    #[sea_orm(has_many = "super::identity_provider_models::Entity")]
    ProviderModels,
    #[sea_orm(has_many = "super::identity_endpoint_models::Entity")]
    EndpointModels,
    #[sea_orm(has_many = "super::identity_endpoint_model_routes::Entity")]
    EndpointModelRoutes,
    #[sea_orm(has_many = "super::identity_response_sessions::Entity")]
    ResponseSessions,
    #[sea_orm(has_many = "super::identity_model_aliases::Entity")]
    ModelAliases,
}

impl Related<super::identities::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Identity.def()
    }
}

impl Related<super::identity_provider_models::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProviderModels.def()
    }
}

impl Related<super::identity_endpoint_models::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EndpointModels.def()
    }
}

impl Related<super::identity_endpoint_model_routes::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EndpointModelRoutes.def()
    }
}

impl Related<super::identity_response_sessions::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ResponseSessions.def()
    }
}

impl Related<super::identity_model_aliases::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ModelAliases.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
