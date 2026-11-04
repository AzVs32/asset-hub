use ah_vfs::domain::{
    DriverError,
    storage::{StorageError, StorageErrorKind},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryDriverError {
    #[error("memory instance configuration is invalid")]
    InvalidData,
}

impl From<MemoryDriverError> for DriverError {
    fn from(error: MemoryDriverError) -> Self {
        match error {
            MemoryDriverError::InvalidData => Self::Storage(StorageError::with_source(
                StorageErrorKind::InvalidData,
                error,
            )),
        }
    }
}
