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
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamStatsSnapshot {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub tool_call_count: i64,
    pub completed: bool,
    pub final_usage_seen: bool,
}

impl StreamStatsSnapshot {
    pub(crate) fn record_event(&mut self, event: &InternalStreamEvent) {
        match event {
            InternalStreamEvent::ToolCallDone { .. } => {
                self.tool_call_count += 1;
            }
            InternalStreamEvent::Usage(usage) => {
                self.input_tokens = usage.input_tokens.and_then(u64_to_i64);
                self.output_tokens = usage.output_tokens.and_then(u64_to_i64);
                self.total_tokens = usage.total_tokens.and_then(u64_to_i64);
                self.cache_read_tokens = usage.cache_read_tokens.and_then(u64_to_i64);
                self.cache_write_tokens = usage.cache_write_tokens.and_then(u64_to_i64);
                self.final_usage_seen = usage.input_tokens.is_some()
                    || usage.output_tokens.is_some()
                    || usage.total_tokens.is_some()
                    || usage.cache_read_tokens.is_some()
                    || usage.cache_write_tokens.is_some();
            }
            InternalStreamEvent::Finished(_) => {
                self.completed = true;
            }
            InternalStreamEvent::TextDelta(_) | InternalStreamEvent::ToolCallDelta { .. } => {}
        }
    }
}

fn u64_to_i64(value: u64) -> Option<i64> {
    i64::try_from(value).ok()
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
        Some("stop") | Some("end_turn") | Some("stop_sequence") => InternalFinishReason::Stop,
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
    pub(crate) finish_reason: Option<InternalFinishReason>,
}

#[cfg(test)]
mod tests {
    use super::{
        internal_finish_reason_from_str, InternalFinishReason, InternalStreamEvent,
        StreamStatsSnapshot, StreamUsage,
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
            cache_read_tokens: Some(2),
            cache_write_tokens: Some(1),
        });

        assert!(matches!(
            event,
            InternalStreamEvent::Usage(StreamUsage {
                input_tokens: Some(3),
                output_tokens: Some(5),
                total_tokens: Some(8),
                cache_read_tokens: Some(2),
                cache_write_tokens: Some(1),
            })
        ));
    }

    #[test]
    fn records_stream_stats_from_internal_events() {
        let mut stats = StreamStatsSnapshot::default();

        stats.record_event(&InternalStreamEvent::Usage(StreamUsage {
            input_tokens: Some(3),
            output_tokens: Some(5),
            total_tokens: Some(8),
            cache_read_tokens: Some(2),
            cache_write_tokens: Some(1),
        }));
        stats.record_event(&InternalStreamEvent::ToolCallDone {
            index: 0,
            id: "call_1".to_string(),
            name: "get_weather".to_string(),
            arguments: "{}".to_string(),
        });
        stats.record_event(&InternalStreamEvent::Finished(
            InternalFinishReason::ToolUse,
        ));

        assert_eq!(stats.input_tokens, Some(3));
        assert_eq!(stats.output_tokens, Some(5));
        assert_eq!(stats.total_tokens, Some(8));
        assert_eq!(stats.cache_read_tokens, Some(2));
        assert_eq!(stats.cache_write_tokens, Some(1));
        assert_eq!(stats.tool_call_count, 1);
        assert!(stats.completed);
        assert!(stats.final_usage_seen);
    }
}
