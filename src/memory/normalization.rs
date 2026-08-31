use sha2::{Digest, Sha256};

use super::MemoryCandidate;
use crate::storage::StorageError;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NormalizedMemoryCandidate {
    pub kind: String,
    pub content: String,
    pub normalized_key: String,
    pub conflict_key: Option<String>,
    pub confidence: f64,
    pub evidence: String,
    pub evidence_hash: String,
    pub observed_at: String,
    pub status: &'static str,
}

pub(crate) fn normalize_candidate(
    candidate: MemoryCandidate,
) -> Result<NormalizedMemoryCandidate, StorageError> {
    let kind = normalize_text(&candidate.kind);
    let content = normalize_text(&candidate.content);
    let evidence = normalize_text(&candidate.evidence);
    if kind.is_empty() || content.is_empty() || evidence.is_empty() {
        return Err(StorageError::ValidationFailed(
            "memory candidates require kind, content, and evidence".to_string(),
        ));
    }
    if !(0.0..=1.0).contains(&candidate.confidence) {
        return Err(StorageError::ValidationFailed(
            "memory confidence must be between zero and one".to_string(),
        ));
    }
    if candidate.observed_at.trim().is_empty() {
        return Err(StorageError::ValidationFailed(
            "memory candidates require an observed time".to_string(),
        ));
    }

    Ok(NormalizedMemoryCandidate {
        normalized_key: hash(&format!("{kind}\n{content}")),
        evidence_hash: hash(&evidence),
        kind,
        content,
        conflict_key: candidate
            .conflict_key
            .map(|value| normalize_text(&value))
            .filter(|value| !value.is_empty()),
        confidence: candidate.confidence,
        evidence,
        observed_at: candidate.observed_at,
        status: if candidate.confidence < 0.75 {
            "pending_review"
        } else {
            "active"
        },
    })
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hash(value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(value.as_bytes());
    format!("{:x}", digest.finalize())
}
