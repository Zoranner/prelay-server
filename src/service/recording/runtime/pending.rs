use chrono::{DateTime, Duration as ChronoDuration, Utc};

use crate::store::{
    recording_journal::{RecordEnvelope, RecordKind, TurnCompletedRecord, TurnFinishedRecord},
    storage::ResponseState,
};

pub(super) fn pending_response_state(envelope: &RecordEnvelope) -> Option<ResponseState> {
    let (state, created_at) = match envelope.kind {
        RecordKind::TurnCompleted => {
            let payload: TurnCompletedRecord = envelope.decode_payload().ok()?;
            (
                payload.response_state?,
                payload
                    .timing
                    .finished_at
                    .or(Some(payload.timing.started_at)),
            )
        }
        RecordKind::TurnFinished => {
            let payload: TurnFinishedRecord = envelope.decode_payload().ok()?;
            (
                payload.response_state?,
                payload
                    .timing
                    .finished_at
                    .or(Some(payload.timing.started_at)),
            )
        }
        RecordKind::TurnStarted | RecordKind::ProviderObservation => return None,
    };
    let created_at = DateTime::parse_from_rfc3339(&created_at?)
        .ok()?
        .with_timezone(&Utc);
    Some(ResponseState {
        identity_id: envelope.identity_id.clone(),
        response_id: state.response_id,
        previous_response_id: state.previous_response_id,
        conversation_id: Some(state.conversation_id),
        provider_id: state.provider_id,
        model: state.model,
        upstream_response_id: state.upstream_response_id,
        request_items: state.request_items,
        response_items: state.response_items,
        created_at: created_at.to_rfc3339(),
        expires_at: (created_at + ChronoDuration::days(30)).to_rfc3339(),
    })
}
