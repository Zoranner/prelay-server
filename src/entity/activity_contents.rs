use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "activity_contents")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub activity_id: String,
    pub input_text: String,
    pub output_text: String,
    pub media_metadata_json: Option<String>,
    pub is_truncated: bool,
    pub content_hash: String,
    pub status: String,
    pub attempts: i64,
    pub next_attempt_at: Option<String>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<String>,
    pub last_error: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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
