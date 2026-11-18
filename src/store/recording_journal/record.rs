use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use super::{journal::JournalRecord, JournalError};

pub(crate) const RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecordKind {
    TurnStarted,
    TurnFinished,
    TurnCompleted,
    ProviderObservation,
}

impl RecordKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::TurnStarted => "turn_started",
            Self::TurnFinished => "turn_finished",
            Self::TurnCompleted => "turn_completed",
            Self::ProviderObservation => "provider_observation",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, JournalError> {
        match value {
            "turn_started" => Ok(Self::TurnStarted),
            "turn_finished" => Ok(Self::TurnFinished),
            "turn_completed" => Ok(Self::TurnCompleted),
            "provider_observation" => Ok(Self::ProviderObservation),
            _ => Err(JournalError::Corrupt(format!(
                "unknown recording kind: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RecordEnvelope {
    pub(crate) schema_version: u32,
    pub(crate) record_id: String,
    pub(crate) activity_id: String,
    pub(crate) identity_id: String,
    pub(crate) kind: RecordKind,
    pub(crate) created_at: String,
    pub(crate) payload: Value,
}

impl RecordEnvelope {
    pub(crate) fn new(
        activity_id: impl Into<String>,
        identity_id: impl Into<String>,
        kind: RecordKind,
        created_at: impl Into<String>,
        payload: Value,
    ) -> Self {
        let activity_id = activity_id.into();
        Self {
            schema_version: RECORD_SCHEMA_VERSION,
            record_id: format!("{activity_id}:{}", kind.as_str()),
            activity_id,
            identity_id: identity_id.into(),
            kind,
            created_at: created_at.into(),
            payload,
        }
    }

    pub(crate) fn from_payload<T: Serialize>(
        activity_id: impl Into<String>,
        identity_id: impl Into<String>,
        kind: RecordKind,
        created_at: impl Into<String>,
        payload: &T,
    ) -> Result<Self, JournalError> {
        let envelope = Self::new(
            activity_id,
            identity_id,
            kind,
            created_at,
            serde_json::to_value(payload)?,
        );
        envelope.validate()?;
        Ok(envelope)
    }

    pub(crate) fn decode_payload<T: DeserializeOwned>(&self) -> Result<T, JournalError> {
        serde_json::from_value(self.payload.clone())
            .map_err(|error| JournalError::Corrupt(format!("invalid recording payload: {error}")))
    }

    fn validate(&self) -> Result<(), JournalError> {
        if self.schema_version != RECORD_SCHEMA_VERSION {
            return Err(JournalError::Corrupt(format!(
                "unsupported recording schema version {}",
                self.schema_version
            )));
        }
        if self.activity_id.is_empty() || self.identity_id.is_empty() {
            return Err(JournalError::Corrupt(
                "recording envelope is missing its activity or identity".to_string(),
            ));
        }
        if self.created_at.is_empty() {
            return Err(JournalError::Corrupt(
                "recording envelope is missing created_at".to_string(),
            ));
        }
        if self.record_id != format!("{}:{}", self.activity_id, self.kind.as_str()) {
            return Err(JournalError::Corrupt(
                "recording envelope record_id does not match its identity".to_string(),
            ));
        }
        Ok(())
    }
}

impl JournalRecord {
    pub(crate) fn from_envelope(
        envelope: RecordEnvelope,
        chunk_bytes: usize,
    ) -> Result<Self, JournalError> {
        if chunk_bytes == 0 {
            return Err(JournalError::InvalidLayout(
                "recording chunk size must be positive".to_string(),
            ));
        }
        envelope.validate()?;
        let serialized = serde_json::to_vec(&envelope)?;
        let parts = serialized
            .chunks(chunk_bytes)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        Ok(Self::new(
            envelope.activity_id,
            envelope.kind.as_str().to_string(),
            parts,
        ))
    }

    pub(crate) fn decode_envelope(&self) -> Result<RecordEnvelope, JournalError> {
        let mut serialized = Vec::new();
        for part in &self.parts {
            serialized.extend_from_slice(part);
        }
        let envelope: RecordEnvelope = serde_json::from_slice(&serialized).map_err(|error| {
            JournalError::Corrupt(format!("invalid recording envelope JSON: {error}"))
        })?;
        envelope.validate()?;
        if envelope.activity_id != self.activity_id {
            return Err(JournalError::Corrupt(
                "recording envelope activity does not match its frame".to_string(),
            ));
        }
        if RecordKind::parse(&self.kind)? != envelope.kind {
            return Err(JournalError::Corrupt(
                "recording envelope kind does not match its frame".to_string(),
            ));
        }
        Ok(envelope)
    }
}
