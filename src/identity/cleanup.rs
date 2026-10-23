pub const INACTIVE_IDENTITY_RETENTION_DAYS: i64 = 90;

use chrono::{Duration, Utc};

use crate::{activity::ActivityStore, storage::StorageError};

pub async fn delete_expired_identities(
    activity_store: &ActivityStore,
) -> Result<u64, StorageError> {
    activity_store
        .delete_inactive_identities(Utc::now(), Duration::days(INACTIVE_IDENTITY_RETENTION_DAYS))
        .await
}
