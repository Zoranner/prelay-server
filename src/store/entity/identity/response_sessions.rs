use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_response_sessions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub response_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub identity_id: String,
    pub previous_response_id: Option<String>,
    pub conversation_id: Option<String>,
    pub provider_id: String,
    pub model: String,
    pub status: String,
    pub protocol_format: String,
    pub upstream_response_id: Option<String>,
    pub request_items_json: Option<String>,
    pub response_items_json: Option<String>,
    pub expires_at: Option<String>,
    pub input_messages_json: String,
    pub output_items_json: String,
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
}

impl Related<super::super::identities::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Identity.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
