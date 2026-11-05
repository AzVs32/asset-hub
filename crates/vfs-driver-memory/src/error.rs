use ah_vfs::domain::{
    DriverError, DriverKeyError,
    storage::{StorageCapabilitiesError, StorageError, StorageErrorKind},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryDriverError {
    #[error("driver key declaration is invalid")]
    DriverKey(#[from] DriverKeyError),
    #[error("storage capability declaration is invalid")]
    Capabilities(#[from] StorageCapabilitiesError),
    #[error("memory instance configuration is invalid")]
    InvalidData,
}

impl From<MemoryDriverError> for DriverError {
    fn from(error: MemoryDriverError) -> Self {
        match error {
            MemoryDriverError::DriverKey(source) => Self::Storage(StorageError::with_source(
                StorageErrorKind::ProtocolViolation,
                source,
            )),
            MemoryDriverError::Capabilities(source) => Self::Storage(StorageError::from(source)),
            MemoryDriverError::InvalidData => Self::Storage(StorageError::with_source(
                StorageErrorKind::InvalidData,
                error,
            )),
        }
    }
}
