use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::RolloutStatus;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventPhase {
    Request,
    Response,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionMetaEvent {
    pub activity_id: String,
    pub session_id: String,
    pub identity_id: String,
    pub protocol_in: String,
    pub protocol_out: String,
    pub protocol_upstream: Option<String>,
    pub endpoint_name: String,
    pub model_requested: String,
    pub model_upstream: String,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MessageEvent {
    pub phase: EventPhase,
    pub role: String,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolCallEvent {
    pub phase: EventPhase,
    pub tool_call_id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolResultEvent {
    pub phase: EventPhase,
    pub tool_call_id: String,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReasoningEvent {
    pub phase: EventPhase,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MediaEvent {
    pub phase: EventPhase,
    pub media_type: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageEvent {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ErrorEvent {
    pub code: Option<String>,
    pub message: String,
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionEndEvent {
    pub status: RolloutStatus,
    pub http_status: Option<u16>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum RolloutEventBody {
    SessionMeta(SessionMetaEvent),
    Message(MessageEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Reasoning(ReasoningEvent),
    Media(MediaEvent),
    Usage(UsageEvent),
    Error(ErrorEvent),
    SessionEnd(SessionEndEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RolloutEvent {
    pub seq: u64,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: Value,
}
