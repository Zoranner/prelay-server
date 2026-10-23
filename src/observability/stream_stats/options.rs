use crate::{
    activity::{RawStreamContentCapture, RolloutWriter},
    bridge::internal::InternalMessage,
    bridge::stream::SharedStreamStats,
};

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
    pub store: bool,
    /// 桥接流在服务端生成；原生 Responses 流留空，由 SSE 的响应 id 填入。
    pub response_id: Option<String>,
}
