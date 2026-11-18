use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    codec::internal::{InternalMessage, InternalResponse},
    domain::{
        activity::{ContentDraft, SessionContext},
        stats::{ActivityInsert, StreamActivityUpdate},
    },
    store::storage::ResponseStateInsert,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TurnTiming {
    pub(crate) started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) first_token_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ResponseStateRecord {
    pub(crate) response_id: String,
    pub(crate) previous_response_id: Option<String>,
    pub(crate) conversation_id: String,
    pub(crate) provider_id: String,
    pub(crate) model: String,
    pub(crate) upstream_response_id: Option<String>,
    pub(crate) request_items: Value,
    pub(crate) response_items: Value,
    pub(crate) input_messages: Vec<InternalMessage>,
    pub(crate) response: InternalResponse,
}

impl ResponseStateRecord {
    pub(crate) fn from_insert(insert: ResponseStateInsert<'_>) -> Self {
        Self {
            response_id: insert.response_id.to_string(),
            previous_response_id: insert.previous_response_id.map(str::to_string),
            conversation_id: insert.conversation_id.to_string(),
            provider_id: insert.provider_id.to_string(),
            model: insert.model.to_string(),
            upstream_response_id: insert.upstream_response_id.map(str::to_string),
            request_items: insert.request_items.clone(),
            response_items: insert.response_items.clone(),
            input_messages: insert.input_messages.to_vec(),
            response: insert.response.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RecordedContentEvent {
    pub(crate) observed_at: String,
    pub(crate) event: ContentDraft,
}

impl RecordedContentEvent {
    pub(crate) fn new(observed_at: impl Into<String>, event: ContentDraft) -> Self {
        Self {
            observed_at: observed_at.into(),
            event,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct TurnStartedRecord {
    pub(crate) activity: ActivityInsert,
    pub(crate) session: SessionContext,
    pub(crate) request_events: Vec<RecordedContentEvent>,
    pub(crate) timing: TurnTiming,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct TurnCompletedRecord {
    pub(crate) activity: ActivityInsert,
    pub(crate) session: SessionContext,
    pub(crate) request_events: Vec<RecordedContentEvent>,
    pub(crate) response_events: Vec<RecordedContentEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) response_state: Option<ResponseStateRecord>,
    pub(crate) timing: TurnTiming,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct TurnFinishedRecord {
    pub(crate) activity: StreamActivityUpdate,
    pub(crate) session: SessionContext,
    pub(crate) response_events: Vec<RecordedContentEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) response_state: Option<ResponseStateRecord>,
    pub(crate) timing: TurnTiming,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProviderObservationRecord {
    pub(crate) endpoint_id: String,
    pub(crate) endpoint_name: String,
    pub(crate) model_name: String,
    pub(crate) provider_id: String,
    pub(crate) observed_at: String,
}
