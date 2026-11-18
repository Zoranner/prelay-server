use std::fmt;

use crate::store::recording_journal::JournalError;

#[derive(Debug)]
pub(crate) enum RecordingRuntimeError {
    Config(String),
    Journal(JournalError),
    Materializer(String),
    Worker(String),
    Capacity(String),
}

impl fmt::Display for RecordingRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => write!(formatter, "recording configuration error: {message}"),
            Self::Journal(error) => error.fmt(formatter),
            Self::Materializer(message) => {
                write!(formatter, "recording materializer error: {message}")
            }
            Self::Worker(message) => write!(formatter, "recording worker error: {message}"),
            Self::Capacity(message) => write!(formatter, "recording capacity error: {message}"),
        }
    }
}

impl std::error::Error for RecordingRuntimeError {}

impl From<JournalError> for RecordingRuntimeError {
    fn from(error: JournalError) -> Self {
        Self::Journal(error)
    }
}
