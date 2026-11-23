use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;

use crate::{
    domain::activity::rollout::{RolloutStatus, TurnContext},
    store::{
        recording_journal::{
            materializer::{MaterializeError, MaterializeSink},
            payload::{
                ProviderObservationRecord, RecordedContentEvent, ResponseStateRecord,
                TurnCompletedRecord, TurnFinishedRecord, TurnStartedRecord,
            },
            pending_response_states::PendingResponseStates,
            record::{RecordEnvelope, RecordKind},
        },
        rollout::{
            CompletedTurnMaterialization, FinishedTurnMaterialization, RolloutStore,
            StartedTurnMaterialization, TimedContentDraft,
        },
        storage::{
            ActivityRolloutMaterialization, CompletedStreamMaterialization, ProtocolAccess,
            ResponseStateInsert, Storage,
        },
    },
};

pub(crate) struct ActivityMaterializeSink {
    storage: Storage,
    rollouts: RolloutStore,
    pending_response_states: PendingResponseStates,
}

impl ActivityMaterializeSink {
    pub(crate) fn new(
        storage: Storage,
        rollouts: RolloutStore,
        pending_response_states: PendingResponseStates,
    ) -> Self {
        Self {
            storage,
            rollouts,
            pending_response_states,
        }
    }
}

impl MaterializeSink for ActivityMaterializeSink {
    fn apply(&self, record: RecordEnvelope) -> BoxFuture<'static, Result<(), MaterializeError>> {
        let storage = self.storage.clone();
        let rollouts = self.rollouts.clone();
        let pending = self.pending_response_states.clone();
        Box::pin(async move { apply_record(storage, rollouts, pending, record).await })
    }
}

async fn apply_record(
    storage: Storage,
    rollouts: RolloutStore,
    pending: PendingResponseStates,
    envelope: RecordEnvelope,
) -> Result<(), MaterializeError> {
    match envelope.kind {
        RecordKind::TurnCompleted => {
            apply_turn_completed(storage, rollouts, pending, envelope).await
        }
        RecordKind::TurnStarted => apply_turn_started(storage, rollouts, envelope).await,
        RecordKind::TurnFinished => apply_turn_finished(storage, rollouts, pending, envelope).await,
        RecordKind::ProviderObservation => apply_provider_observation(storage, envelope).await,
    }
}

async fn apply_provider_observation(
    storage: Storage,
    envelope: RecordEnvelope,
) -> Result<(), MaterializeError> {
    let payload: ProviderObservationRecord = envelope.decode_payload()?;
    let access = ProtocolAccess {
        identity_id: envelope.identity_id.clone(),
        endpoint_id: payload.endpoint_id.clone(),
        endpoint_name: payload.endpoint_name.clone(),
    };
    if let Err(error) = storage
        .remember_protocol_model_provider_at(
            &access,
            &payload.model_name,
            &payload.provider_id,
            &payload.observed_at,
        )
        .await
    {
        // Provider selection is an optimization, not part of the activity record's
        // correctness. A stale preference must not block activity/response materialization.
        tracing::warn!(
            error_code = error.code().as_str(),
            endpoint_id = %payload.endpoint_id,
            model_name = %payload.model_name,
            "failed to materialize provider observation; keeping the previous route"
        );
    }
    Ok(())
}

async fn apply_turn_completed(
    storage: Storage,
    rollouts: RolloutStore,
    pending: PendingResponseStates,
    envelope: RecordEnvelope,
) -> Result<(), MaterializeError> {
    let payload: TurnCompletedRecord = envelope.decode_payload()?;
    let TurnCompletedRecord {
        activity,
        session,
        request_events,
        response_events,
        response_state,
        timing,
    } = payload;
    let started_at = timing.started_at;
    let finished_at = timing.finished_at.ok_or_else(|| {
        MaterializeError::Sink("turn_completed is missing finished_at".to_string())
    })?;
    let status = rollout_status(&activity.status)?;
    let http_status = u16::try_from(activity.http_status).ok();
    let turn = TurnContext {
        activity_id: envelope.activity_id.clone(),
        model_requested: activity.model_requested.clone(),
        model_upstream: activity.model_upstream.clone(),
    };
    let request_events = request_events
        .into_iter()
        .map(|event| TimedContentDraft::new(event.observed_at, event.event))
        .collect();
    let response_events = response_events
        .into_iter()
        .map(|event| TimedContentDraft::new(event.observed_at, event.event))
        .collect();
    let summary = rollouts
        .materialize_completed_turn(
            &envelope.identity_id,
            CompletedTurnMaterialization {
                session,
                turn,
                created_at: started_at.clone(),
                request_events,
                response_events,
                status,
                http_status,
                error_code: activity.error_code.clone(),
                error_message: activity.error_message.clone(),
                finished_at: finished_at.clone(),
            },
        )
        .await
        .map_err(|error| MaterializeError::Sink(error.to_string()))?;
    storage
        .upsert_activity_with_rollout_at(ActivityRolloutMaterialization {
            identity_id: envelope.identity_id.clone(),
            activity_id: envelope.activity_id.clone(),
            activity,
            activity_created_at: started_at.clone(),
            rollout: summary,
            rollout_created_at: started_at,
            completed_at: Some(finished_at.clone()),
        })
        .await
        .map_err(|error| MaterializeError::Sink(error.to_string()))?;
    if let Some(state) = response_state {
        save_response_state(
            &storage,
            &pending,
            &envelope.identity_id,
            state,
            &finished_at,
        )
        .await?;
    }
    Ok(())
}

async fn apply_turn_started(
    storage: Storage,
    rollouts: RolloutStore,
    envelope: RecordEnvelope,
) -> Result<(), MaterializeError> {
    let payload: TurnStartedRecord = envelope.decode_payload()?;
    let TurnStartedRecord {
        activity,
        session,
        request_events,
        timing,
    } = payload;
    let started_at = timing.started_at;
    let turn = TurnContext {
        activity_id: envelope.activity_id.clone(),
        model_requested: activity.model_requested.clone(),
        model_upstream: activity.model_upstream.clone(),
    };
    let started = rollouts
        .materialize_started_turn(
            &envelope.identity_id,
            StartedTurnMaterialization {
                session,
                turn,
                created_at: started_at.clone(),
                request_events: timed_events(request_events),
            },
        )
        .await
        .map_err(|error| MaterializeError::Sink(error.to_string()))?;
    // rollout 的 turn_end 只能说明线程文件写过结束标记，不能证明活动行已经落库：
    // 上一次物化可能刚写完 turn_end 就中断。这里始终按幂等 upsert 补齐活动行。
    let summary = started.summary(envelope.activity_id.clone());
    storage
        .upsert_activity_with_rollout_at(ActivityRolloutMaterialization {
            identity_id: envelope.identity_id.clone(),
            activity_id: envelope.activity_id.clone(),
            activity,
            activity_created_at: started_at.clone(),
            rollout: summary,
            rollout_created_at: started_at,
            completed_at: None,
        })
        .await
        .map_err(|error| MaterializeError::Sink(error.to_string()))?;
    Ok(())
}

async fn apply_turn_finished(
    storage: Storage,
    rollouts: RolloutStore,
    pending: PendingResponseStates,
    envelope: RecordEnvelope,
) -> Result<(), MaterializeError> {
    let payload: TurnFinishedRecord = envelope.decode_payload()?;
    let TurnFinishedRecord {
        activity,
        session,
        response_events,
        response_state,
        activity_snapshot,
        timing,
    } = payload;
    let started_at = timing.started_at;
    let finished_at = timing.finished_at.ok_or_else(|| {
        MaterializeError::Sink("turn_finished is missing finished_at".to_string())
    })?;
    let status = rollout_status(&activity.status)?;
    let summary = rollouts
        .materialize_finished_turn(
            &envelope.identity_id,
            FinishedTurnMaterialization {
                session,
                activity_id: envelope.activity_id.clone(),
                response_events: timed_events(response_events),
                status,
                http_status: u16::try_from(activity.http_status).ok(),
                error_code: activity.error_code.clone(),
                error_message: activity.error_message.clone(),
                finished_at: finished_at.clone(),
            },
        )
        .await
        .map_err(|error| MaterializeError::Sink(error.to_string()))?;
    storage
        .complete_stream_activity_with_rollout_at(CompletedStreamMaterialization {
            identity_id: envelope.identity_id.clone(),
            activity_id: envelope.activity_id.clone(),
            activity,
            activity_snapshot,
            rollout: summary,
            rollout_created_at: started_at,
            completed_at: finished_at.clone(),
        })
        .await
        .map_err(|error| MaterializeError::Sink(error.to_string()))?;
    if let Some(state) = response_state {
        save_response_state(
            &storage,
            &pending,
            &envelope.identity_id,
            state,
            &finished_at,
        )
        .await?;
    }
    Ok(())
}

async fn save_response_state(
    storage: &Storage,
    pending: &PendingResponseStates,
    identity_id: &str,
    state: ResponseStateRecord,
    finished_at: &str,
) -> Result<(), MaterializeError> {
    let created_at = DateTime::parse_from_rfc3339(finished_at)
        .map_err(|error| MaterializeError::Sink(format!("invalid finished_at timestamp: {error}")))?
        .with_timezone(&Utc);
    let insert = ResponseStateInsert {
        identity_id,
        response_id: &state.response_id,
        previous_response_id: state.previous_response_id.as_deref(),
        conversation_id: &state.conversation_id,
        provider_id: &state.provider_id,
        model: &state.model,
        upstream_response_id: state.upstream_response_id.as_deref(),
        request_items: &state.request_items,
        response_items: &state.response_items,
    };
    storage
        .save_response_state_at(insert, created_at, created_at + Duration::days(30))
        .await
        .map_err(|error| MaterializeError::Sink(error.to_string()))?;
    pending.remove(identity_id, &state.response_id);
    Ok(())
}

fn timed_events(events: Vec<RecordedContentEvent>) -> Vec<TimedContentDraft> {
    events
        .into_iter()
        .map(|event| TimedContentDraft::new(event.observed_at, event.event))
        .collect()
}

fn rollout_status(status: &str) -> Result<RolloutStatus, MaterializeError> {
    match status {
        "success" => Ok(RolloutStatus::Completed),
        "failed" => Ok(RolloutStatus::Failed),
        "partial" => Ok(RolloutStatus::Partial),
        other => Err(MaterializeError::Sink(format!(
            "unsupported activity status: {other}"
        ))),
    }
}
