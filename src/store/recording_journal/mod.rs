mod checkpoint;
mod cipher;
mod frame;
mod group_commit;
mod journal;
mod layout;
mod materializer;
mod payload;
mod pending_response_states;
mod reader;
mod record;
mod sink;

#[cfg(test)]
mod tests;

pub(crate) use cipher::JournalCipher;
pub(crate) use group_commit::{GroupCommitConfig, GroupCommitHandle, GroupCommitter};
pub(crate) use journal::{JournalRecord, RecordingJournalWriter};
pub(crate) use materializer::Materializer;
pub(crate) use payload::{
    ProviderObservationRecord, RecordedContentEvent, ResponseStateRecord, TurnCompletedRecord,
    TurnFinishedRecord, TurnStartedRecord, TurnTiming,
};
pub(crate) use pending_response_states::PendingResponseStates;
pub(crate) use record::{RecordEnvelope, RecordKind};
pub(crate) use sink::ActivityMaterializeSink;

use std::{fmt, io};

#[derive(Debug)]
pub(crate) enum JournalError {
    Io(io::Error),
    Json(serde_json::Error),
    Crypto(String),
    Corrupt(String),
    InvalidLayout(String),
    Locked(String),
    Unhealthy(String),
    EmptyRecord,
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "journal IO error: {error}"),
            Self::Json(error) => write!(formatter, "journal JSON error: {error}"),
            Self::Crypto(message) => write!(formatter, "journal crypto error: {message}"),
            Self::Corrupt(message) => write!(formatter, "journal corruption: {message}"),
            Self::InvalidLayout(message) => write!(formatter, "journal layout error: {message}"),
            Self::Locked(message) => write!(formatter, "journal is locked: {message}"),
            Self::Unhealthy(message) => write!(formatter, "journal is unhealthy: {message}"),
            Self::EmptyRecord => write!(formatter, "journal record has no parts"),
        }
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Crypto(_)
            | Self::Corrupt(_)
            | Self::InvalidLayout(_)
            | Self::Locked(_)
            | Self::Unhealthy(_)
            | Self::EmptyRecord => None,
        }
    }
}

impl From<io::Error> for JournalError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for JournalError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
