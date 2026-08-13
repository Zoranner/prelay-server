pub const INACTIVE_IDENTITY_RETENTION_DAYS: i64 = 90;

use chrono::{Duration, Utc};

use crate::storage::{Storage, StorageError};

pub async fn delete_expired_identities(storage: &Storage) -> Result<u64, StorageError> {
    storage
        .delete_inactive_identities(Utc::now(), Duration::days(INACTIVE_IDENTITY_RETENTION_DAYS))
        .await
}
