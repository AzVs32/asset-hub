use ah_vfs::domain::{
    DriverError,
    storage::{StorageCapabilitiesError, StorageError, StorageErrorKind},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryDriverError {
    #[error("storage capability declaration is invalid")]
    Capabilities(#[from] StorageCapabilitiesError),
    #[error("memory instance configuration is invalid")]
    InvalidData,
}

impl From<MemoryDriverError> for DriverError {
    fn from(error: MemoryDriverError) -> Self {
        match error {
            MemoryDriverError::Capabilities(source) => Self::Storage(StorageError::from(source)),
            MemoryDriverError::InvalidData => Self::Storage(StorageError::with_source(
                StorageErrorKind::InvalidData,
                error,
            )),
        }
    }
}
