use crate::{
    codec::bridge::stream::SharedStreamStats, codec::internal::InternalMessage,
    domain::activity::RawStreamContentCapture, store::rollout::RolloutWriter,
};
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum RecordingMode {
    Required,
    BestEffort,
}

pub(super) struct StreamRecordOptions {
    pub(super) stats: Option<SharedStreamStats>,
    pub(super) content_capture: Option<RawStreamContentCapture>,
    pub(super) rollout: Option<RolloutWriter>,
    pub(super) session: Option<StreamSession>,
    pub(super) log_id: String,
    pub(super) mode: RecordingMode,
}

/// 流结束时写入 `identity_response_sessions` 所需的上下文。
pub(crate) struct StreamSession {
    pub previous_response_id: Option<String>,
    pub conversation_id: String,
    pub provider_id: String,
    pub model: String,
    pub input_messages: Vec<InternalMessage>,
    pub request_items: Vec<Value>,
    /// 客户端可见的本地响应 id；上游 id 只从流统计中读取。
    pub response_id: String,
}
