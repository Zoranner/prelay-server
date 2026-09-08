use prelay_server::{
    activity::{
        ActivityContentDraft, ActivityContentPolicy, NormalizedActivityContent,
        RawStreamContentCapture, RawStreamProtocol,
    },
    entity::activity_contents,
    identity::credential::generate_credential,
    schema::initialize,
    stats::ActivityInsert,
    storage::{MasterKey, Storage},
};
use sea_orm::{ColumnTrait, Database, EntityTrait, QueryFilter};
use std::time::Duration;

#[test]
fn activity_content_policy_uses_a_positive_configured_size_limit() {
    assert_eq!(
        ActivityContentPolicy::from_values(None)
            .expect("default content policy")
            .max_bytes,
        64 * 1024
    );
    assert_eq!(
        ActivityContentPolicy::from_values(Some("1024"))
            .expect("configured content policy")
            .max_bytes,
        1024
    );
    assert!(ActivityContentPolicy::from_values(Some("0")).is_err());
    assert!(ActivityContentPolicy::from_values(Some("invalid")).is_err());
}

#[test]
fn raw_stream_capture_drops_an_oversized_event_and_accepts_the_next_one() {
    let mut capture = RawStreamContentCapture::new(RawStreamProtocol::ImageGeneration);

    capture.observe_chunk(&vec![b'x'; 256 * 1024]);
    capture.observe_chunk(b"\n\ndata: {\"type\":\"image_generation.completed\"}\n\n");
    capture.finish();

    assert!(capture.is_truncated());
    assert!(capture.is_completed());
    assert!(capture.output_text().is_empty());
}

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

#[tokio::test]
async fn recovers_stale_capturing_content_for_terminal_activities() {
    let (storage, db) = test_storage().await;
    let identity_id = register_identity(&storage).await;
    let activity_id = "stale-capturing-activity".to_string();
    let activity = ActivityInsert {
        status: "success".to_string(),
        ..Default::default()
    };
    storage
        .start_stream_activity(
            &identity_id,
            &activity_id,
            activity,
            NormalizedActivityContent {
                input_text: "preserve input".to_string(),
                output_text: "preserve output".to_string(),
                media_metadata_json: None,
                is_truncated: false,
                content_hash: "preserve-hash".to_string(),
            },
        )
        .await
        .expect("persist capturing content");

    let recovered = storage
        .recover_stale_activity_content(Duration::ZERO)
        .await
        .expect("recover stale content");

    assert_eq!(recovered, 1);
    let row = activity_contents::Entity::find()
        .filter(activity_contents::Column::ActivityId.eq(activity_id))
        .one(&db)
        .await
        .expect("load recovered content")
        .expect("recovered content exists");
    assert_eq!(row.status, "pending");
    assert!(row.completed_at.is_some());
    assert_eq!(row.input_text, "preserve input");
    assert_eq!(row.output_text, "preserve output");
    assert!(row.lease_owner.is_none());
    assert!(row.lease_expires_at.is_none());
}

#[tokio::test]
async fn does_not_recover_capturing_content_for_an_active_activity() {
    let (storage, db) = test_storage().await;
    let identity_id = register_identity(&storage).await;
    let activity_id = "active-capturing-activity".to_string();
    let activity = ActivityInsert {
        status: "capturing".to_string(),
        ..Default::default()
    };
    storage
        .start_stream_activity(
            &identity_id,
            &activity_id,
            activity,
            NormalizedActivityContent {
                input_text: "active input".to_string(),
                output_text: "active output".to_string(),
                media_metadata_json: None,
                is_truncated: false,
                content_hash: "active-hash".to_string(),
            },
        )
        .await
        .expect("persist capturing content");

    let recovered = storage
        .recover_stale_activity_content(Duration::from_secs(60))
        .await
        .expect("recover stale content");

    assert_eq!(recovered, 0);
    let row = activity_contents::Entity::find()
        .filter(activity_contents::Column::ActivityId.eq(activity_id))
        .one(&db)
        .await
        .expect("load active content")
        .expect("active content exists");
    assert_eq!(row.status, "capturing");
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
