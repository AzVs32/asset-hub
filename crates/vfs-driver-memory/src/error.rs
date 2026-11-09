use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
use ah_vfs::domain::DriverKeyError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryDriverError {
    #[error("driver key declaration is invalid")]
    DriverKey(#[from] DriverKeyError),
    #[error("memory instance configuration is invalid")]
    InvalidData,
}

impl From<MemoryDriverError> for DriverError {
    fn from(error: MemoryDriverError) -> Self {
        match error {
            MemoryDriverError::DriverKey(source) => Self::Lifecycle(LifecycleError::with_source(
                LifecycleFailure::InvalidDeclaration,
                source,
            )),
            MemoryDriverError::InvalidData => Self::Lifecycle(LifecycleError::with_source(
                LifecycleFailure::InvalidRecord,
                error,
            )),
        }
    }
}
