use serde_json::json;

use crate::store::recording_journal::{
    journal::JournalRecord,
    record::{RecordEnvelope, RecordKind, RECORD_SCHEMA_VERSION},
    JournalError,
};

fn envelope() -> RecordEnvelope {
    RecordEnvelope::new(
        "activity-1",
        "identity-1",
        RecordKind::TurnCompleted,
        "2026-11-16T12:00:00+08:00",
        json!({
            "message": "你好，world",
            "usage": {"input_tokens": 3, "output_tokens": 5}
        }),
    )
}

#[test]
fn round_trips_an_envelope_across_multiple_parts() {
    let original = envelope();
    let record = JournalRecord::from_envelope(original.clone(), 7).expect("encode envelope");
    assert!(record.parts.len() > 1);
    assert_eq!(record.activity_id, "activity-1");
    assert_eq!(record.kind, "turn_completed");
    assert_eq!(record.decode_envelope().expect("decode envelope"), original);
}

#[test]
fn rejects_a_zero_chunk_size() {
    assert!(matches!(
        JournalRecord::from_envelope(envelope(), 0),
        Err(JournalError::InvalidLayout(_))
    ));
}

#[test]
fn rejects_an_unsupported_schema_version() {
    let mut envelope = envelope();
    envelope.schema_version = RECORD_SCHEMA_VERSION + 1;
    assert!(matches!(
        JournalRecord::from_envelope(envelope, 64),
        Err(JournalError::Corrupt(_))
    ));
}

#[test]
fn rejects_a_record_id_that_does_not_match_the_envelope() {
    let mut envelope = envelope();
    envelope.record_id = "activity-1:turn_started".to_string();
    assert!(matches!(
        JournalRecord::from_envelope(envelope, 64),
        Err(JournalError::Corrupt(_))
    ));
}

#[test]
fn rejects_frame_metadata_that_does_not_match_the_envelope() {
    let mut activity_mismatch =
        JournalRecord::from_envelope(envelope(), 64).expect("encode envelope");
    activity_mismatch.activity_id = "activity-2".to_string();
    assert!(matches!(
        activity_mismatch.decode_envelope(),
        Err(JournalError::Corrupt(_))
    ));

    let mut kind_mismatch = JournalRecord::from_envelope(envelope(), 64).expect("encode envelope");
    kind_mismatch.kind = "turn_started".to_string();
    assert!(matches!(
        kind_mismatch.decode_envelope(),
        Err(JournalError::Corrupt(_))
    ));
}

#[test]
fn rejects_an_empty_payload_identity() {
    let mut envelope = envelope();
    envelope.identity_id.clear();
    assert!(matches!(
        JournalRecord::from_envelope(envelope, 64),
        Err(JournalError::Corrupt(_))
    ));
}
