use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    domain::{
        activity::{ContentDraft, SessionContext},
        stats::ActivityInsert,
    },
    store::{
        recording_journal::{
            ProviderObservationRecord, RecordEnvelope, RecordKind, RecordedContentEvent,
            ResponseStateRecord, TurnCompletedRecord, TurnTiming,
        },
        storage::{ResponseStateInsert, StorageError},
    },
    AppState,
};

pub(crate) struct CompletedTurnRecord {
    pub(crate) identity_id: String,
    pub(crate) activity: ActivityInsert,
    pub(crate) session: SessionContext,
    pub(crate) request_events: Vec<ContentDraft>,
    pub(crate) response_events: Vec<ContentDraft>,
    pub(crate) response_state: Option<ResponseStateRecord>,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) finished_at: DateTime<Utc>,
}

pub(crate) async fn record_completed_turn(
    state: &AppState,
    turn: CompletedTurnRecord,
) -> Result<(), StorageError> {
    let CompletedTurnRecord {
        identity_id,
        activity,
        session,
        request_events,
        response_events,
        response_state,
        started_at,
        finished_at,
    } = turn;
    if let Some(recording) = &state.recording {
        let activity_id = Uuid::new_v4().to_string();
        let started_at = started_at.to_rfc3339();
        let finished_at = finished_at.to_rfc3339();
        let payload = TurnCompletedRecord {
            activity,
            session: session.clone(),
            request_events: timed_events(request_events, &started_at),
            response_events: timed_events(response_events, &finished_at),
            response_state,
            timing: TurnTiming {
                started_at: started_at.clone(),
                first_token_at: None,
                finished_at: Some(finished_at),
            },
        };
        let envelope = RecordEnvelope::from_payload(
            activity_id,
            &identity_id,
            RecordKind::TurnCompleted,
            started_at,
            &payload,
        )
        .map_err(|error| StorageError::Rollout(error.to_string()))?;
        return recording
            .commit_envelope(envelope)
            .await
            .map_err(|error| StorageError::Recording(error.to_string()));
    }

    if let Some(response_state) = response_state.as_ref() {
        state
            .activity_store
            .save_response_state(ResponseStateInsert {
                identity_id: &identity_id,
                response_id: &response_state.response_id,
                previous_response_id: response_state.previous_response_id.as_deref(),
                conversation_id: &response_state.conversation_id,
                provider_id: &response_state.provider_id,
                model: &response_state.model,
                upstream_response_id: response_state.upstream_response_id.as_deref(),
                request_items: &response_state.request_items,
                response_items: &response_state.response_items,
            })
            .await?;
    }
    state
        .activity_store
        .record_completed(
            &identity_id,
            activity,
            &session,
            request_events,
            response_events,
        )
        .await
}

pub(crate) async fn record_provider_observation_best_effort(
    state: &AppState,
    access: &crate::store::storage::ProtocolAccess,
    model_name: &str,
    provider_id: &str,
) {
    let Some(recording) = state.recording.clone() else {
        if let Err(error) = state
            .storage
            .remember_protocol_model_provider(access, model_name, provider_id)
            .await
        {
            tracing::warn!(
                error_code = error.code().as_str(),
                "failed to remember active endpoint model provider"
            );
        }
        return;
    };
    let observed_at = Utc::now().to_rfc3339();
    let activity_id = format!(
        "observation:{}:{}:{}:{}",
        access.endpoint_id,
        model_name,
        provider_id,
        Uuid::new_v4()
    );
    let payload = ProviderObservationRecord {
        endpoint_id: access.endpoint_id.clone(),
        endpoint_name: access.endpoint_name.clone(),
        model_name: model_name.to_string(),
        provider_id: provider_id.to_string(),
        observed_at: observed_at.clone(),
    };
    let envelope = match RecordEnvelope::from_payload(
        activity_id,
        access.identity_id.clone(),
        RecordKind::ProviderObservation,
        observed_at,
        &payload,
    ) {
        Ok(envelope) => envelope,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "failed to build provider observation record"
            );
            return;
        }
    };
    tokio::spawn(async move {
        if let Err(error) = recording.commit_envelope(envelope).await {
            tracing::warn!(
                error = %error,
                "failed to commit provider observation record"
            );
        }
    });
}

fn timed_events(events: Vec<ContentDraft>, observed_at: &str) -> Vec<RecordedContentEvent> {
    events
        .into_iter()
        .map(|event| RecordedContentEvent::new(observed_at, event))
        .collect()
}
