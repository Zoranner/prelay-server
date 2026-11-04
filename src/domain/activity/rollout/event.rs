use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::RolloutStatus;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationFormat {
    /// 首行为 v1 文件头。
    V1,
    /// 没有 v1 文件头，按旧行格式读取；续写后仍保留该标记，尾部可能包含新追加的 v1 事件。
    Legacy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationMeta {
    pub v: u8,
    #[serde(rename = "type")]
    pub kind: String,
    pub thread_id: String,
    pub created_at: String,
}

impl ConversationMeta {
    pub fn new(thread_id: String, created_at: String) -> Self {
        Self {
            v: 1,
            kind: "meta".to_string(),
            thread_id,
            created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnEvent {
    pub turn_id: String,
    pub activity_id: String,
    pub model: String,
    pub upstream_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextEvent {
    pub refs: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnEndEvent {
    pub status: RolloutStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ConversationEventBody {
    Window(WindowEvent),
    Turn(TurnEvent),
    Message(ConversationMessage),
    Reasoning(ConversationReasoning),
    ToolCall(ConversationToolCall),
    ToolResult(ConversationToolResult),
    Usage(ConversationUsage),
    Error(ConversationError),
    TurnEnd(TurnEndEvent),
    Context(ContextEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationEvent {
    pub seq: u64,
    pub time: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContentEvent {
    pub seq: u64,
    pub time: String,
    pub body: ContentEventBody,
}

impl ConversationEvent {
    pub fn content_event(&self) -> Option<ContentEvent> {
        if !matches!(
            self.event_type.as_str(),
            "message" | "reasoning" | "tool_call" | "tool_result" | "error"
        ) {
            return None;
        }
        let body = serde_json::from_value::<ContentEventBody>(serde_json::json!({
            "type": self.event_type,
            "payload": self.payload,
        }))
        .ok()?;
        Some(ContentEvent {
            seq: self.seq,
            time: self.time.clone(),
            body,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventPhase {
    Request,
    Response,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationMessage {
    pub phase: EventPhase,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_phase: Option<String>,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationReasoning {
    pub phase: EventPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationToolCall {
    pub phase: EventPhase,
    pub tool_call_id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationToolResult {
    pub phase: EventPhase,
    pub tool_call_id: String,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationError {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ContentEventBody {
    Message(ConversationMessage),
    Reasoning(ConversationReasoning),
    ToolCall(ConversationToolCall),
    ToolResult(ConversationToolResult),
    Usage(ConversationUsage),
    Error(ConversationError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentDraft {
    pub item_id: Option<String>,
    pub body: ContentEventBody,
}

impl ContentDraft {
    pub fn new(body: ContentEventBody) -> Self {
        Self {
            item_id: None,
            body,
        }
    }

    pub fn with_item_id(mut self, item_id: impl Into<String>) -> Self {
        self.item_id = Some(item_id.into());
        self
    }
}
