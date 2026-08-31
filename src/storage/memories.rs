use chrono::Utc;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};
use uuid::Uuid;

use crate::{
    entity::{memories, memory_sources},
    memory::{normalize_candidate, MemoryCandidate, MemoryRecord, MemorySearch, MemorySource},
};

use super::{Storage, StorageError};

impl Storage {
    pub async fn upsert_memory(
        &self,
        identity_id: &str,
        candidate: MemoryCandidate,
    ) -> Result<MemoryRecord, StorageError> {
        upsert(&self.db, identity_id, candidate).await
    }

    pub async fn search_memories(
        &self,
        memory_search: MemorySearch,
    ) -> Result<Vec<MemoryRecord>, StorageError> {
        search(&self.db, memory_search).await
    }
}

async fn upsert(
    db: &DatabaseConnection,
    identity_id: &str,
    candidate: MemoryCandidate,
) -> Result<MemoryRecord, StorageError> {
    let candidate = normalize_candidate(candidate)?;
    let transaction = db.begin().await?;
    let now = Utc::now().to_rfc3339();
    let memory = memories::Entity::find()
        .filter(memories::Column::NormalizedKey.eq(&candidate.normalized_key))
        .one(&transaction)
        .await?;
    let memory_id = match memory {
        Some(memory) => memory.id,
        None => {
            let id = Uuid::new_v4().to_string();
            memories::ActiveModel {
                id: Set(id.clone()),
                normalized_key: Set(candidate.normalized_key.clone()),
                conflict_key: Set(candidate.conflict_key.clone()),
                kind: Set(candidate.kind.clone()),
                status: Set(candidate.status.to_string()),
                content: Set(candidate.content.clone()),
                confidence: Set(candidate.confidence),
                created_at: Set(now.clone()),
                updated_at: Set(now.clone()),
            }
            .insert(&transaction)
            .await?;
            id
        }
    };

    let source_exists = memory_sources::Entity::find()
        .filter(memory_sources::Column::MemoryId.eq(&memory_id))
        .filter(memory_sources::Column::IdentityId.eq(identity_id))
        .filter(memory_sources::Column::EvidenceHash.eq(&candidate.evidence_hash))
        .filter(memory_sources::Column::ObservedAt.eq(&candidate.observed_at))
        .one(&transaction)
        .await?
        .is_some();
    if !source_exists {
        memory_sources::ActiveModel {
            id: Set(Uuid::new_v4().to_string()),
            memory_id: Set(memory_id.clone()),
            identity_id: Set(identity_id.to_string()),
            evidence: Set(candidate.evidence),
            evidence_hash: Set(candidate.evidence_hash),
            observed_at: Set(candidate.observed_at),
            created_at: Set(now),
        }
        .insert(&transaction)
        .await?;
    }

    if let Some(conflict_key) = candidate.conflict_key {
        let conflicts = memories::Entity::find()
            .filter(memories::Column::ConflictKey.eq(&conflict_key))
            .all(&transaction)
            .await?;
        if conflicts.len() > 1 {
            memories::Entity::update_many()
                .col_expr(memories::Column::Status, Expr::value("conflicted"))
                .col_expr(
                    memories::Column::UpdatedAt,
                    Expr::value(Utc::now().to_rfc3339()),
                )
                .filter(memories::Column::ConflictKey.eq(conflict_key))
                .exec(&transaction)
                .await?;
        }
    }

    transaction.commit().await?;
    memory_by_id(db, &memory_id).await
}

async fn search(
    db: &DatabaseConnection,
    search: MemorySearch,
) -> Result<Vec<MemoryRecord>, StorageError> {
    let mut query = memories::Entity::find();
    if let Some(query_text) = search
        .query
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        query = query.filter(memories::Column::Content.contains(query_text.trim()));
    }
    if let Some(kind) = search
        .kind
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        query = query.filter(memories::Column::Kind.eq(kind.trim()));
    }
    if let Some(status) = search
        .status
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        query = query.filter(memories::Column::Status.eq(status.trim()));
    }
    if let Some(created_after) = search.created_after.as_deref() {
        query = query.filter(memories::Column::CreatedAt.gte(created_after));
    }
    if let Some(created_before) = search.created_before.as_deref() {
        query = query.filter(memories::Column::CreatedAt.lte(created_before));
    }
    if let Some(identity_id) = search.identity_id.as_deref() {
        let ids = memory_sources::Entity::find()
            .filter(memory_sources::Column::IdentityId.eq(identity_id))
            .all(db)
            .await?
            .into_iter()
            .map(|source| source.memory_id)
            .collect::<Vec<_>>();
        query = query.filter(memories::Column::Id.is_in(ids));
    }

    let rows = query
        .order_by_desc(memories::Column::UpdatedAt)
        .limit(search.limit.unwrap_or(100).min(100) as u64)
        .all(db)
        .await?;
    let mut records = Vec::with_capacity(rows.len());
    for memory in rows {
        records.push(record_from_model(db, memory).await?);
    }
    Ok(records)
}

async fn memory_by_id(db: &DatabaseConnection, id: &str) -> Result<MemoryRecord, StorageError> {
    let memory = memories::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(StorageError::MemoryNotFound)?;
    record_from_model(db, memory).await
}

async fn record_from_model(
    db: &DatabaseConnection,
    memory: memories::Model,
) -> Result<MemoryRecord, StorageError> {
    let sources = memory_sources::Entity::find()
        .filter(memory_sources::Column::MemoryId.eq(&memory.id))
        .order_by_asc(memory_sources::Column::ObservedAt)
        .all(db)
        .await?
        .into_iter()
        .map(|source| MemorySource {
            identity_id: source.identity_id,
            evidence: source.evidence,
            observed_at: source.observed_at,
        })
        .collect();
    Ok(MemoryRecord {
        id: memory.id,
        normalized_key: memory.normalized_key,
        conflict_key: memory.conflict_key,
        kind: memory.kind,
        status: memory.status,
        content: memory.content,
        confidence: memory.confidence,
        created_at: memory.created_at,
        updated_at: memory.updated_at,
        sources,
    })
}
