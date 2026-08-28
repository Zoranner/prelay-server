use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identities")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub machine_id: String,
    pub account_sid: String,
    pub credential_hash: String,
    pub display_name: String,
    pub created_at: String,
    pub last_active_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::identity::provider_configs::Entity")]
    ProviderConfigs,
    #[sea_orm(has_many = "super::identity::endpoint_configs::Entity")]
    EndpointConfigs,
    #[sea_orm(has_many = "super::identity::response_sessions::Entity")]
    ResponseSessions,
    #[sea_orm(has_many = "super::identity::request_logs::Entity")]
    RequestLogs,
    #[sea_orm(has_many = "super::identity::model_aliases::Entity")]
    ModelAliases,
}

impl Related<super::identity::provider_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProviderConfigs.def()
    }
}

impl Related<super::identity::endpoint_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EndpointConfigs.def()
    }
}

impl Related<super::identity::response_sessions::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ResponseSessions.def()
    }
}

impl Related<super::identity::request_logs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RequestLogs.def()
    }
}

impl Related<super::identity::model_aliases::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ModelAliases.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
