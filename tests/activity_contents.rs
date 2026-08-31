use prelay_server::{
    activity::ActivityContentDraft,
    entity::activity_contents,
    identity::credential::generate_credential,
    schema::initialize,
    stats::ActivityInsert,
    storage::{MasterKey, Storage},
};
use sea_orm::{ColumnTrait, Database, EntityTrait, QueryFilter};

#[tokio::test]
async fn enqueues_one_pending_content_for_a_persisted_activity() {
    let (storage, db) = test_storage().await;
    let identity_id = register_identity(&storage).await;
    let activity_id = storage
        .insert_activity(&identity_id, ActivityInsert::default())
        .await
        .expect("persist activity");
    let draft = ActivityContentDraft {
        activity_id: activity_id.clone(),
        input_text: "input".to_string(),
        output_text: "output".to_string(),
        media_metadata_json: None,
        is_truncated: false,
        content_hash: "content-hash".to_string(),
    };

    storage
        .enqueue_activity_content(draft.clone())
        .await
        .expect("enqueue activity content");

    let row = activity_contents::Entity::find()
        .filter(activity_contents::Column::ActivityId.eq(activity_id))
        .one(&db)
        .await
        .expect("load activity content")
        .expect("stored activity content");
    assert_eq!(row.input_text, "input");
    assert_eq!(row.output_text, "output");
    assert_eq!(row.status, "pending");
    assert_eq!(row.attempts, 0);
    assert!(row.next_attempt_at.is_some());
    assert!(storage.enqueue_activity_content(draft).await.is_err());
}

async fn test_storage() -> (Storage, sea_orm::DatabaseConnection) {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("connect SQLite");
    initialize(&db).await.expect("initialize schema");
    (
        Storage::from_connection(db.clone(), MasterKey::from_bytes([0; 32])),
        db,
    )
}

async fn register_identity(storage: &Storage) -> String {
    storage
        .register_identity("machine-content", "sid-content", &generate_credential())
        .await
        .expect("register identity")
        .identity_id
}
