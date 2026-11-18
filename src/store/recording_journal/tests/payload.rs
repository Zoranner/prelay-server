use serde_json::json;

use crate::{
    codec::internal::InternalResponse,
    domain::{
        activity::{ContentDraft, ContentEventBody, ConversationUsage, SessionContext},
        stats::{ActivityInsert, StreamActivityUpdate},
    },
    store::recording_journal::{
        journal::JournalRecord,
        payload::{
            ProviderObservationRecord, RecordedContentEvent, ResponseStateRecord,
            TurnCompletedRecord, TurnFinishedRecord, TurnStartedRecord, TurnTiming,
        },
        record::{RecordEnvelope, RecordKind},
        JournalError,
    },
};

fn activity() -> ActivityInsert {
    ActivityInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: "chat_completions".to_string(),
        endpoint_name: "endpoint-a".to_string(),
        provider_id: "provider-a".to_string(),
        provider_name: "Provider A".to_string(),
        provider_type: "openai".to_string(),
        model_requested: "model-requested".to_string(),
        model_upstream: "model-upstream".to_string(),
        status: "success".to_string(),
        http_status: 200,
        input_tokens: Some(10),
        output_tokens: Some(4),
        latency_ms: 120,
        ..Default::default()
    }
}

fn timing(first_token: bool) -> TurnTiming {
    TurnTiming {
        started_at: "2026-11-17T12:00:00.000+08:00".to_string(),
        first_token_at: first_token.then(|| "2026-11-17T12:00:00.100+08:00".to_string()),
        finished_at: Some("2026-11-17T12:00:01.000+08:00".to_string()),
    }
}

fn response_state() -> ResponseStateRecord {
    ResponseStateRecord {
        response_id: "resp_local".to_string(),
        previous_response_id: Some("resp_previous".to_string()),
        conversation_id: "conversation-a".to_string(),
        provider_id: "provider-a".to_string(),
        model: "model-a".to_string(),
        upstream_response_id: Some("resp_upstream".to_string()),
        request_items: json!([{"role": "user", "content": "hello"}]),
        response_items: json!([{"type": "message"}]),
        input_messages: Vec::new(),
        response: InternalResponse {
            id: "resp_local".to_string(),
            model: "model-a".to_string(),
            output: Vec::new(),
            usage: None,
        },
    }
}

fn usage_event() -> ContentDraft {
    ContentDraft::new(ContentEventBody::Usage(ConversationUsage {
        input_tokens: Some(10),
        output_tokens: Some(4),
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
    }))
}

fn recorded_usage_event(observed_at: &str) -> RecordedContentEvent {
    RecordedContentEvent::new(observed_at, usage_event())
}

fn round_trip<T>(kind: RecordKind, payload: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let envelope = RecordEnvelope::from_payload(
        "activity-1",
        "identity-1",
        kind,
        "2026-11-17T12:00:00.000+08:00",
        payload,
    )
    .expect("build envelope");
    let record = JournalRecord::from_envelope(envelope.clone(), 11).expect("encode record");
    let decoded = record.decode_envelope().expect("decode envelope");
    assert_eq!(decoded, envelope);
    let decoded_payload = decoded.decode_payload::<T>().expect("decode payload");
    assert_eq!(&decoded_payload, payload);
    decoded_payload
}

#[test]
fn round_trips_turn_completed() {
    let payload = TurnCompletedRecord {
        activity: activity(),
        session: SessionContext::with_window(
            "thread-a".to_string(),
            Some("window-a".to_string()),
            2,
        ),
        request_events: vec![recorded_usage_event("2026-11-17T12:00:00.000+08:00")],
        response_events: vec![recorded_usage_event("2026-11-17T12:00:01.000+08:00")],
        response_state: Some(response_state()),
        timing: timing(false),
    };
    assert_eq!(round_trip(RecordKind::TurnCompleted, &payload), payload);
}

#[test]
fn round_trips_turn_started_and_finished() {
    let started = TurnStartedRecord {
        activity: activity(),
        session: SessionContext::new("thread-a"),
        request_events: vec![recorded_usage_event("2026-11-17T12:00:00.000+08:00")],
        timing: TurnTiming {
            started_at: "2026-11-17T12:00:00.000+08:00".to_string(),
            first_token_at: None,
            finished_at: None,
        },
    };
    round_trip(RecordKind::TurnStarted, &started);

    let finished = TurnFinishedRecord {
        activity: StreamActivityUpdate {
            status: "success".to_string(),
            http_status: 200,
            input_tokens: Some(10),
            output_tokens: Some(4),
            latency_ms: 400,
            ..Default::default()
        },
        session: SessionContext::new("thread-a"),
        response_events: vec![recorded_usage_event("2026-11-17T12:00:01.000+08:00")],
        response_state: None,
        timing: timing(true),
    };
    round_trip(RecordKind::TurnFinished, &finished);
}

#[test]
fn round_trips_provider_observation() {
    let payload = ProviderObservationRecord {
        endpoint_id: "endpoint-a".to_string(),
        endpoint_name: "Endpoint A".to_string(),
        model_name: "model-a".to_string(),
        provider_id: "provider-a".to_string(),
        observed_at: "2026-11-17T12:00:01.000+08:00".to_string(),
    };
    round_trip(RecordKind::ProviderObservation, &payload);
}

#[test]
fn rejects_payloads_that_do_not_match_the_kind() {
    let envelope = RecordEnvelope::from_payload(
        "activity-1",
        "identity-1",
        RecordKind::ProviderObservation,
        "2026-11-17T12:00:00.000+08:00",
        &json!({"unexpected": true}),
    )
    .expect("build envelope");
    let record = JournalRecord::from_envelope(envelope, 64).expect("encode record");
    let decoded = record.decode_envelope().expect("decode envelope");
    assert!(matches!(
        decoded.decode_payload::<ProviderObservationRecord>(),
        Err(JournalError::Corrupt(_))
    ));
}
