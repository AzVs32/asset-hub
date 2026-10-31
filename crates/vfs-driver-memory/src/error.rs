use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryDriverError {
    #[error("stored memory instance data is invalid")]
    InvalidData,
    #[error(transparent)]
    Registry(#[from] DriverError),
}

impl From<MemoryDriverError> for DriverError {
    fn from(error: MemoryDriverError) -> Self {
        match error {
            MemoryDriverError::Registry(error) => error,
            MemoryDriverError::InvalidData => Self::Storage(StorageError::with_source(
                StorageErrorKind::InvalidData,
                error,
            )),
        }
    }
}
