// These payload types must be `pub` so external callers can match CoreError.
// Their module is crate-private, so callers cannot import the types directly.

use std::error::Error;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DriverError {
    #[error("driver path is invalid for this driver")]
    InvalidDriverPath,
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
