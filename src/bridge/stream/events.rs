#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalStreamEvent {
    TextDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    ToolCallDone {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    },
    Usage(StreamUsage),
    Finished(InternalFinishReason),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalFinishReason {
    Stop,
    Length,
    ToolUse,
    ContentFilter,
    Error,
    Unknown,
}

pub(crate) fn internal_finish_reason_from_str(reason: Option<&str>) -> InternalFinishReason {
    match reason {
        Some("stop") | Some("end_turn") => InternalFinishReason::Stop,
        Some("length") | Some("max_tokens") => InternalFinishReason::Length,
        Some("tool_calls") | Some("tool_use") => InternalFinishReason::ToolUse,
        Some("content_filter") => InternalFinishReason::ContentFilter,
        Some("error") => InternalFinishReason::Error,
        _ => InternalFinishReason::Unknown,
    }
}

#[derive(Default)]
pub(crate) struct ChatToolCallState {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments: String,
    pub(crate) added: bool,
    pub(crate) done: bool,
}

pub(crate) struct ChatSseEvent {
    pub(crate) text_delta: Option<String>,
    pub(crate) tool_call_deltas: Vec<ChatToolCallDelta>,
    pub(crate) finish_reason: Option<InternalFinishReason>,
}

impl ChatSseEvent {
    pub(crate) fn finished(&self) -> bool {
        self.finish_reason.is_some()
    }

    pub(crate) fn to_internal_events(&self) -> Vec<InternalStreamEvent> {
        let mut events = Vec::new();
        if let Some(delta) = &self.text_delta {
            events.push(InternalStreamEvent::TextDelta(delta.clone()));
        }
        for delta in &self.tool_call_deltas {
            events.push(InternalStreamEvent::ToolCallDelta {
                index: delta.index,
                id: delta.id.clone(),
                name: delta.name.clone(),
                arguments: delta.arguments.clone(),
            });
        }
        if let Some(reason) = self.finish_reason {
            events.push(InternalStreamEvent::Finished(reason));
        }
        events
    }
}

pub(crate) struct ChatToolCallDelta {
    pub(crate) index: usize,
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) arguments: Option<String>,
}

pub(crate) struct AnthropicMessagesSseEvent {
    pub(crate) text_delta: Option<String>,
    pub(crate) finished: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        internal_finish_reason_from_str, InternalFinishReason, InternalStreamEvent, StreamUsage,
    };

    #[test]
    fn maps_known_finish_reasons_to_internal_finish_reason() {
        assert_eq!(
            internal_finish_reason_from_str(Some("stop")),
            InternalFinishReason::Stop
        );
        assert_eq!(
            internal_finish_reason_from_str(Some("length")),
            InternalFinishReason::Length
        );
        assert_eq!(
            internal_finish_reason_from_str(Some("tool_calls")),
            InternalFinishReason::ToolUse
        );
        assert_eq!(
            internal_finish_reason_from_str(Some("content_filter")),
            InternalFinishReason::ContentFilter
        );
        assert_eq!(
            internal_finish_reason_from_str(Some("error")),
            InternalFinishReason::Error
        );
        assert_eq!(
            internal_finish_reason_from_str(Some("unexpected")),
            InternalFinishReason::Unknown
        );
    }

    #[test]
    fn carries_stream_usage_as_internal_event() {
        let event = InternalStreamEvent::Usage(StreamUsage {
            input_tokens: Some(3),
            output_tokens: Some(5),
            total_tokens: Some(8),
        });

        assert!(matches!(
            event,
            InternalStreamEvent::Usage(StreamUsage {
                input_tokens: Some(3),
                output_tokens: Some(5),
                total_tokens: Some(8),
            })
        ));
    }
}
