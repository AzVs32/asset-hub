use std::error::Error;

use thiserror::Error;

/// Errors from driver validation and backend operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DriverError {
    #[error(
        "driver kind must be non-empty and contain only lowercase ASCII letters, digits, `.`, `-`, or `_`"
    )]
    InvalidKind,
    #[error("driver kind is {length} bytes, exceeding the {max}-byte limit")]
    KindTooLong { length: usize, max: usize },

    #[error("driver path is invalid for this driver")]
    InvalidPath,
    #[error("entry was not found")]
    NotFound,
    #[error("entry is not a directory")]
    NotDirectory,
    #[error("entry is a directory")]
    IsDirectory,
    #[error("driver-native name cannot be represented in the virtual namespace")]
    UnrepresentableName,
    #[error("driver backend operation failed: {0}")]
    Backend(#[source] Box<dyn Error + Send + Sync>),
}

impl DriverError {
    /// Preserves the source of a driver backend failure.
    pub fn backend(error: impl Error + Send + Sync + 'static) -> Self {
        Self::Backend(Box::new(error))
    }
}
