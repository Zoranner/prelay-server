use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "identity_activities")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub identity_id: String,
    pub created_at: String,
    pub protocol_in: Option<String>,
    pub protocol_out: Option<String>,
    pub protocol_upstream: Option<String>,
    pub endpoint_name: Option<String>,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub model_requested: Option<String>,
    pub model_upstream: Option<String>,
    pub proxy_token_id: Option<String>,
    pub status: String,
    pub http_status: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub is_streaming: Option<bool>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub estimated_cost: Option<f64>,
    pub currency: Option<String>,
    pub latency_ms: Option<i64>,
    pub upstream_latency_ms: Option<i64>,
    pub first_token_ms: Option<i64>,
    pub tool_call_count: Option<i64>,
    pub upstream_request_id: Option<String>,
    pub metadata_json: Option<String>,
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
