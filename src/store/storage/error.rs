use crate::error::AppError;

use super::StorageError;

impl From<StorageError> for AppError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::Recording(message) => Self::RecordingUnavailable(message),
            error => Self::Protocol {
                code: error.code(),
                message: error.to_string(),
            },
        }
    }
}
