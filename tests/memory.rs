use prelay_server::{
    memory::{MemoryCandidate, MemorySearch},
    storage::Storage,
};

#[tokio::test]
async fn exact_candidates_are_idempotent_and_keep_each_identity_source() {
    let storage = test_storage().await;
    let identity_a = register_identity(&storage, "a").await;
    let identity_b = register_identity(&storage, "b").await;
    let candidate = MemoryCandidate {
        kind: "preference".to_string(),
        content: "团队优先使用 PostgreSQL".to_string(),
        conflict_key: Some("database:preference".to_string()),
        confidence: 0.9,
        evidence: "团队优先使用 PostgreSQL".to_string(),
        observed_at: "2026-08-31T00:00:00Z".to_string(),
    };

    storage
        .upsert_memory(&identity_a, candidate.clone())
        .await
        .expect("store first candidate");
    storage
        .upsert_memory(&identity_a, candidate.clone())
        .await
        .expect("reprocess identical candidate");
    storage
        .upsert_memory(&identity_b, candidate)
        .await
        .expect("store second identity source");

    let memories = storage
        .search_memories(MemorySearch::default())
        .await
        .expect("search memories");
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].status, "active");
    assert_eq!(memories[0].sources.len(), 2);
    assert!(memories[0]
        .sources
        .iter()
        .any(|source| source.identity_id == identity_a));
    assert!(memories[0]
        .sources
        .iter()
        .any(|source| source.identity_id == identity_b));
}

#[tokio::test]
async fn low_confidence_and_conflicting_candidates_remain_reviewable() {
    let storage = test_storage().await;
    let identity = register_identity(&storage, "review").await;
    storage
        .upsert_memory(
            &identity,
            MemoryCandidate {
                kind: "rule".to_string(),
                content: "部署前需要人工复核".to_string(),
                conflict_key: None,
                confidence: 0.4,
                evidence: "可能需要人工复核".to_string(),
                observed_at: "2026-08-31T00:00:00Z".to_string(),
            },
        )
        .await
        .expect("store low confidence candidate");
    storage
        .upsert_memory(
            &identity,
            MemoryCandidate {
                kind: "fact".to_string(),
                content: "部署默认数据库是 PostgreSQL 14".to_string(),
                conflict_key: Some("deployment:default_database".to_string()),
                confidence: 0.9,
                evidence: "部署配置显示 PostgreSQL 14".to_string(),
                observed_at: "2026-08-31T00:00:00Z".to_string(),
            },
        )
        .await
        .expect("store first fact");
    storage
        .upsert_memory(
            &identity,
            MemoryCandidate {
                kind: "fact".to_string(),
                content: "部署默认数据库是 PostgreSQL 16".to_string(),
                conflict_key: Some("deployment:default_database".to_string()),
                confidence: 0.9,
                evidence: "部署配置显示 PostgreSQL 16".to_string(),
                observed_at: "2026-08-31T00:01:00Z".to_string(),
            },
        )
        .await
        .expect("store conflicting fact");

    let memories = storage
        .search_memories(MemorySearch::default())
        .await
        .expect("search memories");
    assert!(memories
        .iter()
        .any(|memory| memory.content == "部署前需要人工复核" && memory.status == "pending_review"));
    assert_eq!(
        memories
            .iter()
            .filter(|memory| memory.conflict_key.as_deref() == Some("deployment:default_database"))
            .filter(|memory| memory.status == "conflicted")
            .count(),
        2
    );
}

async fn test_storage() -> Storage {
    prelay_server::test_support::test_storage().await
}

async fn register_identity(storage: &Storage, suffix: &str) -> String {
    storage
        .register_identity(
            &format!("machine-{suffix}"),
            &format!("sid-{suffix}"),
            &prelay_server::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity")
        .identity_id
}
