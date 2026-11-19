use sea_orm::{
    sea_query::OnConflict, ActiveModelTrait, ActiveValue::Set, ConnectionTrait, EntityTrait,
};

use crate::{domain::stats::ActivityInsert, store::entity::identity::activities};

use super::StorageError;

pub(super) async fn insert_with_id_at<C>(
    db: &C,
    identity_id: &str,
    id: String,
    log: ActivityInsert,
    created_at: String,
) -> Result<(), StorageError>
where
    C: ConnectionTrait,
{
    model(identity_id, id, log, created_at).insert(db).await?;
    Ok(())
}

pub(super) async fn upsert_with_id_at<C>(
    db: &C,
    identity_id: &str,
    id: String,
    log: ActivityInsert,
    created_at: String,
) -> Result<(), StorageError>
where
    C: ConnectionTrait,
{
    activities::Entity::insert(model(identity_id, id, log, created_at))
        .on_conflict(
            OnConflict::column(activities::Column::Id)
                .update_columns([
                    activities::Column::IdentityId,
                    activities::Column::CreatedAt,
                    activities::Column::ProtocolIn,
                    activities::Column::ProtocolOut,
                    activities::Column::ProtocolUpstream,
                    activities::Column::EndpointName,
                    activities::Column::ProviderId,
                    activities::Column::ProviderName,
                    activities::Column::ProviderType,
                    activities::Column::ModelRequested,
                    activities::Column::ModelUpstream,
                    activities::Column::ProxyTokenId,
                    activities::Column::Status,
                    activities::Column::HttpStatus,
                    activities::Column::ErrorCode,
                    activities::Column::ErrorMessage,
                    activities::Column::IsStreaming,
                    activities::Column::InputTokens,
                    activities::Column::OutputTokens,
                    activities::Column::ReasoningTokens,
                    activities::Column::CacheReadTokens,
                    activities::Column::CacheWriteTokens,
                    activities::Column::LatencyMs,
                    activities::Column::UpstreamLatencyMs,
                    activities::Column::FirstTokenMs,
                    activities::Column::ToolCallCount,
                    activities::Column::UpstreamRequestId,
                    activities::Column::MetadataJson,
                ])
                .to_owned(),
        )
        .exec(db)
        .await?;
    Ok(())
}

pub(super) fn model(
    identity_id: &str,
    id: String,
    log: ActivityInsert,
    created_at: String,
) -> activities::ActiveModel {
    activities::ActiveModel {
        id: Set(id),
        identity_id: Set(identity_id.to_string()),
        created_at: Set(created_at),
        protocol_in: Set(Some(log.protocol_in)),
        protocol_out: Set(Some(log.protocol_out)),
        protocol_upstream: Set(Some(log.protocol_upstream)),
        endpoint_name: Set(Some(log.endpoint_name)),
        provider_id: Set(Some(log.provider_id)),
        provider_name: Set(Some(log.provider_name)),
        provider_type: Set(Some(log.provider_type)),
        model_requested: Set(Some(log.model_requested)),
        model_upstream: Set(Some(log.model_upstream)),
        proxy_token_id: Set(None),
        status: Set(log.status),
        http_status: Set(Some(log.http_status)),
        error_code: Set(log.error_code),
        error_message: Set(log.error_message),
        is_streaming: Set(Some(log.is_streaming)),
        input_tokens: Set(log.input_tokens),
        output_tokens: Set(log.output_tokens),
        reasoning_tokens: Set(log.reasoning_tokens),
        cache_read_tokens: Set(log.cache_read_tokens),
        cache_write_tokens: Set(log.cache_write_tokens),
        latency_ms: Set(Some(log.latency_ms)),
        upstream_latency_ms: Set(log.upstream_latency_ms),
        first_token_ms: Set(log.first_token_ms),
        tool_call_count: Set(log.tool_call_count),
        upstream_request_id: Set(log.upstream_request_id),
        metadata_json: Set(None),
    }
}
