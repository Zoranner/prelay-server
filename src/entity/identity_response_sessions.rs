use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_response_sessions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub response_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub identity_id: String,
    pub previous_response_id: Option<String>,
    pub provider_id: String,
    pub model: String,
    pub input_messages_json: String,
    pub output_items_json: String,
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
    #[sea_orm(
        belongs_to = "super::identity_provider_configs::Entity",
        from = "Column::ProviderId",
        to = "super::identity_provider_configs::Column::Id"
    )]
    Provider,
}

impl Related<super::identities::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Identity.def()
    }
}

impl Related<super::identity_provider_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Provider.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
