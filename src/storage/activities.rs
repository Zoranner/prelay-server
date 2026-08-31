use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use uuid::Uuid;

use crate::{
    entity::identity::activities,
    stats::{
        estimate_cost, load_model_prices, ActivityInsert, ActivitySummary, ModelPrice,
        StreamActivityUpdate,
    },
};

use super::{Storage, StorageError};

impl Storage {
    pub async fn insert_activity(
        &self,
        identity_id: &str,
        log: ActivityInsert,
    ) -> Result<String, StorageError> {
        insert(&self.db, identity_id, log).await
    }

    pub async fn insert_activity_with_id(
        &self,
        identity_id: &str,
        id: String,
        log: ActivityInsert,
    ) -> Result<(), StorageError> {
        insert_with_id(&self.db, identity_id, id, log).await
    }

    pub async fn update_stream_activity(
        &self,
        identity_id: &str,
        id: &str,
        update: StreamActivityUpdate,
    ) -> Result<(), StorageError> {
        update_stream(&self.db, identity_id, id, update).await
    }

    pub async fn list_activities(
        &self,
        identity_id: &str,
        limit: usize,
    ) -> Result<Vec<ActivitySummary>, StorageError> {
        list(&self.db, identity_id, limit).await
    }
}

async fn insert(
    db: &DatabaseConnection,
    identity_id: &str,
    log: ActivityInsert,
) -> Result<String, StorageError> {
    let id = Uuid::new_v4().to_string();
    insert_with_id(db, identity_id, id.clone(), log).await?;
    Ok(id)
}

async fn insert_with_id(
    db: &DatabaseConnection,
    identity_id: &str,
    id: String,
    log: ActivityInsert,
) -> Result<(), StorageError> {
    let prices = load_model_prices().unwrap_or_default();
    insert_with_id_and_prices(db, identity_id, id, log, &prices).await
}

async fn insert_with_id_and_prices(
    db: &DatabaseConnection,
    identity_id: &str,
    id: String,
    log: ActivityInsert,
    prices: &[ModelPrice],
) -> Result<(), StorageError> {
    let cost = estimate_cost(&log, prices);
    activities::ActiveModel {
        id: Set(id),
        identity_id: Set(identity_id.to_string()),
        created_at: Set(Utc::now().to_rfc3339()),
        protocol_in: Set(Some(log.protocol_in)),
        protocol_out: Set(Some(log.protocol_out)),
        protocol_upstream: Set(Some(log.protocol_upstream)),
        endpoint_name: Set(Some(log.endpoint_name)),
        provider_id: Set(Some(log.provider_id)),
        provider_name: Set(Some(log.provider_name)),
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
        estimated_cost: Set(cost.as_ref().map(|cost| cost.estimated_cost)),
        currency: Set(cost.map(|cost| cost.currency)),
        latency_ms: Set(Some(log.latency_ms)),
        upstream_latency_ms: Set(log.upstream_latency_ms),
        first_token_ms: Set(log.first_token_ms),
        tool_call_count: Set(log.tool_call_count),
        upstream_request_id: Set(log.upstream_request_id),
        metadata_json: Set(log.metadata_json),
    }
    .insert(db)
    .await?;
    Ok(())
}

async fn update_stream(
    db: &DatabaseConnection,
    identity_id: &str,
    id: &str,
    update: StreamActivityUpdate,
) -> Result<(), StorageError> {
    let prices = load_model_prices().unwrap_or_default();
    update_stream_with_prices(db, identity_id, id, update, &prices).await
}

async fn update_stream_with_prices(
    db: &DatabaseConnection,
    identity_id: &str,
    id: &str,
    update: StreamActivityUpdate,
    prices: &[ModelPrice],
) -> Result<(), StorageError> {
    let row = activities::Entity::find_by_id(id)
        .filter(activities::Column::IdentityId.eq(identity_id))
        .one(db)
        .await?
        .ok_or(StorageError::ActivityNotFound)?;
    let pricing_log = ActivityInsert {
        provider_name: row.provider_name.clone().unwrap_or_default(),
        model_requested: row.model_requested.clone().unwrap_or_default(),
        model_upstream: row.model_upstream.clone().unwrap_or_default(),
        status: update.status.clone(),
        http_status: update.http_status,
        error_code: update.error_code.clone(),
        error_message: update.error_message.clone(),
        is_streaming: true,
        input_tokens: update.input_tokens,
        output_tokens: update.output_tokens,
        reasoning_tokens: update.reasoning_tokens,
        cache_read_tokens: update.cache_read_tokens,
        cache_write_tokens: update.cache_write_tokens,
        latency_ms: update.latency_ms,
        tool_call_count: update.tool_call_count,
        upstream_request_id: update.upstream_request_id.clone(),
        metadata_json: update.metadata_json.clone(),
        ..Default::default()
    };
    let cost = estimate_cost(&pricing_log, prices);
    let mut active: activities::ActiveModel = row.into();
    active.status = Set(update.status);
    active.http_status = Set(Some(update.http_status));
    active.error_code = Set(update.error_code);
    active.error_message = Set(update.error_message);
    active.input_tokens = Set(update.input_tokens);
    active.output_tokens = Set(update.output_tokens);
    active.reasoning_tokens = Set(update.reasoning_tokens);
    active.cache_read_tokens = Set(update.cache_read_tokens);
    active.cache_write_tokens = Set(update.cache_write_tokens);
    active.estimated_cost = Set(cost.as_ref().map(|cost| cost.estimated_cost));
    active.currency = Set(cost.map(|cost| cost.currency));
    active.latency_ms = Set(Some(update.latency_ms));
    active.tool_call_count = Set(update.tool_call_count);
    if update.upstream_request_id.is_some() {
        active.upstream_request_id = Set(update.upstream_request_id);
    }
    active.metadata_json = Set(update.metadata_json);
    active.update(db).await?;
    Ok(())
}

async fn list(
    db: &DatabaseConnection,
    identity_id: &str,
    limit: usize,
) -> Result<Vec<ActivitySummary>, StorageError> {
    let rows = activities::Entity::find()
        .filter(activities::Column::IdentityId.eq(identity_id))
        .order_by_desc(activities::Column::CreatedAt)
        .limit(limit.min(500) as u64)
        .all(db)
        .await?;
    Ok(rows.into_iter().map(request_summary).collect())
}

fn request_summary(row: activities::Model) -> ActivitySummary {
    ActivitySummary {
        id: row.id,
        created_at: row.created_at,
        protocol_in: row.protocol_in,
        protocol_upstream: row.protocol_upstream,
        endpoint_name: row.endpoint_name,
        provider_name: row.provider_name,
        model_requested: row.model_requested,
        model_upstream: row.model_upstream,
        status: row.status,
        http_status: row.http_status,
        error_code: row.error_code,
        error_message: row.error_message,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        is_streaming: row.is_streaming,
        first_token_ms: row.first_token_ms,
        cache_read_tokens: row.cache_read_tokens,
        cache_write_tokens: row.cache_write_tokens,
        latency_ms: row.latency_ms,
        upstream_request_id: row.upstream_request_id,
        metadata_json: row.metadata_json,
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ColumnTrait, Database, EntityTrait, QueryFilter};

    use super::{insert_with_id_and_prices, update_stream_with_prices};
    use crate::{
        entity::identity::activities,
        schema::initialize,
        stats::{ActivityInsert, ModelPrice, StreamActivityUpdate},
        storage::{MasterKey, Storage, StorageError},
    };

    #[tokio::test]
    async fn stream_update_changes_one_existing_identity_log_without_inserting() {
        let (storage, db) = test_storage().await;
        let identity = register_identity(&storage, "stream").await;
        let other_identity = register_identity(&storage, "other").await;
        storage
            .insert_activity_with_id(&identity, "stream-log".to_string(), test_log(None, None))
            .await
            .expect("insert stream log");

        let error = storage
            .update_stream_activity(
                &other_identity,
                "stream-log",
                StreamActivityUpdate {
                    status: "success".to_string(),
                    http_status: 200,
                    input_tokens: Some(99),
                    ..Default::default()
                },
            )
            .await
            .expect_err("another identity cannot update the log");
        assert!(matches!(error, StorageError::ActivityNotFound));

        storage
            .update_stream_activity(
                &identity,
                "stream-log",
                StreamActivityUpdate {
                    status: "success".to_string(),
                    http_status: 200,
                    input_tokens: Some(11),
                    output_tokens: Some(7),
                    latency_ms: 80,
                    upstream_request_id: Some("upstream-stream".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("update stream log");

        let rows = activities::Entity::find()
            .filter(activities::Column::Id.eq("stream-log"))
            .all(&db)
            .await
            .expect("load stream log");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity_id, identity);
        assert_eq!(rows[0].input_tokens, Some(11));
        assert_eq!(rows[0].output_tokens, Some(7));
        assert_eq!(rows[0].latency_ms, Some(80));
        assert_eq!(
            rows[0].upstream_request_id.as_deref(),
            Some("upstream-stream")
        );
    }

    #[tokio::test]
    async fn regular_insert_and_stream_completion_calculate_model_price() {
        let (storage, db) = test_storage().await;
        let identity = register_identity(&storage, "prices").await;
        let prices = [ModelPrice {
            provider: "Provider One".to_string(),
            model: "model-1".to_string(),
            input_price_per_1m: Some(1.0),
            output_price_per_1m: Some(2.0),
            currency: "USD".to_string(),
        }];
        insert_with_id_and_prices(
            &db,
            &identity,
            "regular-price".to_string(),
            test_log(Some(1_000_000), Some(500_000)),
            &prices,
        )
        .await
        .expect("insert priced regular log");
        insert_with_id_and_prices(
            &db,
            &identity,
            "stream-price".to_string(),
            test_log(None, None),
            &prices,
        )
        .await
        .expect("insert stream log");
        update_stream_with_prices(
            &db,
            &identity,
            "stream-price",
            StreamActivityUpdate {
                status: "success".to_string(),
                http_status: 200,
                input_tokens: Some(1_000_000),
                output_tokens: Some(500_000),
                ..Default::default()
            },
            &prices,
        )
        .await
        .expect("complete priced stream log");

        let rows = activities::Entity::find()
            .filter(activities::Column::IdentityId.eq(identity))
            .all(&db)
            .await
            .expect("load priced logs");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| {
            row.estimated_cost == Some(2.0) && row.currency.as_deref() == Some("USD")
        }));
    }

    async fn test_storage() -> (Storage, sea_orm::DatabaseConnection) {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect test database");
        initialize(&db).await.expect("initialize test schema");
        (
            Storage::from_connection(db.clone(), MasterKey::from_bytes([0; 32])),
            db,
        )
    }

    async fn register_identity(storage: &Storage, suffix: &str) -> String {
        storage
            .register_identity(
                &format!("machine-{suffix}"),
                &format!("sid-{suffix}"),
                &crate::identity::credential::generate_credential(),
            )
            .await
            .expect("register identity")
            .identity_id
    }

    fn test_log(input_tokens: Option<i64>, output_tokens: Option<i64>) -> ActivityInsert {
        ActivityInsert {
            protocol_in: "responses".to_string(),
            protocol_out: "responses".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            endpoint_name: String::new(),
            provider_id: "provider-1".to_string(),
            provider_name: "Provider One".to_string(),
            model_requested: "model-1".to_string(),
            model_upstream: "model-1".to_string(),
            status: "success".to_string(),
            http_status: 200,
            error_code: None,
            error_message: None,
            is_streaming: false,
            input_tokens,
            output_tokens,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            latency_ms: 10,
            upstream_latency_ms: None,
            first_token_ms: None,
            tool_call_count: None,
            upstream_request_id: None,
            metadata_json: None,
        }
    }
}
