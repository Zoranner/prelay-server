use crate::error::AppError;

use super::StorageError;

impl From<StorageError> for AppError {
    fn from(error: StorageError) -> Self {
        Self::Protocol {
            code: error.code(),
            message: error.to_string(),
        }
    }
}
