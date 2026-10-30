use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_provider_shares")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub provider_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub grantee_identity_id: String,
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
    #[sea_orm(
        belongs_to = "super::super::identities::Entity",
        from = "Column::GranteeIdentityId",
        to = "super::super::identities::Column::Id"
    )]
    GranteeIdentity,
}

impl Related<super::provider_configs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Provider.def()
    }
}

impl Related<super::super::identities::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GranteeIdentity.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
