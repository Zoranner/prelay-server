use std::{fmt, io};

use serde::Serialize;

mod event;

pub use event::{
    ErrorEvent, EventPhase, MediaEvent, MessageEvent, ReasoningEvent, RolloutEvent,
    RolloutEventBody, SessionEndEvent, SessionMetaEvent, ToolCallEvent, ToolResultEvent,
    UsageEvent, WindowStartEvent,
};

pub const DEFAULT_ACTIVITY_ROLLOUT_DIR: &str = "data/sessions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    pub thread_id: String,
    pub window_id: Option<String>,
    pub generation: u64,
    /// Whether `input` is expected to replay the stored conversation history.
    ///
    /// Native Responses requests with `previous_response_id` only carry the
    /// current turn, so their events must be appended even when they match
    /// stored content. Bridge paths rebuild the full input before recording.
    pub replay_history: bool,
}

impl SessionContext {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            window_id: None,
            generation: 0,
            replay_history: true,
        }
    }

    pub fn with_window(
        thread_id: impl Into<String>,
        window_id: Option<String>,
        generation: u64,
    ) -> Self {
        Self {
            thread_id: thread_id.into(),
            window_id,
            generation,
            replay_history: true,
        }
    }

    pub fn with_replay_history(mut self, replay_history: bool) -> Self {
        self.replay_history = replay_history;
        self
    }
}

#[derive(Debug)]
pub enum RolloutError {
    Io(io::Error),
    Serialize(serde_json::Error),
    InvalidPath(String),
    PartialLine {
        path: std::path::PathBuf,
        valid_events: usize,
    },
}

impl fmt::Display for RolloutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "rollout file error: {error}"),
            Self::Serialize(error) => write!(formatter, "rollout serialization error: {error}"),
            Self::InvalidPath(message) => write!(formatter, "invalid rollout path: {message}"),
            Self::PartialLine { path, valid_events } => write!(
                formatter,
                "rollout file ends with an incomplete line: {} ({valid_events} valid events)",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RolloutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialize(error) => Some(error),
            Self::InvalidPath(_) | Self::PartialLine { .. } => None,
        }
    }
}

impl From<io::Error> for RolloutError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RolloutError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialize(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutStatus {
    Writing,
    Completed,
    Failed,
    Partial,
}

impl RolloutStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Writing => "writing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Partial => "partial",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutSummary {
    pub activity_id: String,
    pub thread_id: String,
    pub window_id: Option<String>,
    pub generation: u64,
    pub relative_path: String,
    pub status: RolloutStatus,
    pub start_line: u64,
    pub end_line: u64,
    pub event_count: u64,
    pub byte_size: u64,
    pub sha256: String,
}
