use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "activity_rollouts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub activity_id: String,
    pub identity_id: String,
    pub session_id: String,
    pub rollout_path: String,
    pub status: String,
    pub event_count: i64,
    pub byte_size: i64,
    pub sha256: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::identity::activities::Entity",
        from = "Column::ActivityId",
        to = "super::identity::activities::Column::Id"
    )]
    Activity,
}

impl Related<super::identity::activities::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Activity.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
