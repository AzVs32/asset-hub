use std::error::Error;

use crate::domain::internal::{DriverKindError, MountIdError, NamespaceError};
use crate::port::internal::DriverError;
use crate::service::internal::MountServiceError;
use thiserror::Error;

/// The public error boundary for core operations.
///
/// Component errors are available through `source()` and are not exported as
/// standalone types. External driver implementations construct contract errors
/// with the `driver_*` methods and inspect them with the `is_*` methods.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CoreError {
    #[error("namespace error: {0}")]
    Namespace(#[from] NamespaceError),
    #[error("driver kind error: {0}")]
    DriverKind(#[from] DriverKindError),
    #[error("mount ID error: {0}")]
    MountId(#[from] MountIdError),
    #[error("driver error: {0}")]
    Driver(#[from] DriverError),
    #[error("mount service error: {0}")]
    MountService(#[from] MountServiceError),
}

impl CoreError {
    /// Constructs a driver error when the driver-specific root path is invalid.
    pub fn driver_invalid_path() -> Self {
        DriverError::InvalidDriverPath.into()
    }

    /// Returns whether the driver-specific root path is invalid.
    pub fn is_invalid_driver_path(&self) -> bool {
        matches!(self, Self::Driver(DriverError::InvalidDriverPath))
    }

    /// Constructs a driver error when the requested entry is absent.
    pub fn driver_not_found() -> Self {
        DriverError::NotFound.into()
    }

    /// Returns whether the requested entry is absent.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Driver(DriverError::NotFound))
    }

    /// Constructs a driver error when the requested directory is a file.
    pub fn driver_not_directory() -> Self {
        DriverError::NotDirectory.into()
    }

    /// Returns whether the requested directory is a file.
    pub fn is_not_directory(&self) -> bool {
        matches!(self, Self::Driver(DriverError::NotDirectory))
    }

    /// Constructs a driver error when the requested file is a directory.
    pub fn driver_is_directory() -> Self {
        DriverError::IsDirectory.into()
    }

    /// Returns whether the requested file is a directory.
    pub fn is_directory(&self) -> bool {
        matches!(self, Self::Driver(DriverError::IsDirectory))
    }

    /// Constructs a driver error when a native entry name cannot be represented in the virtual namespace.
    pub fn driver_unrepresentable_name() -> Self {
        DriverError::UnrepresentableName.into()
    }

    /// Returns whether a native entry name cannot be represented in the virtual namespace.
    pub fn is_unrepresentable_name(&self) -> bool {
        matches!(self, Self::Driver(DriverError::UnrepresentableName))
    }

    /// Preserves the source of a driver backend failure.
    pub fn backend(error: impl Error + Send + Sync + 'static) -> Self {
        DriverError::Backend(Box::new(error)).into()
    }
}
