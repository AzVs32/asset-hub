use std::error::Error;

use thiserror::Error;

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum DriverKindError {
    #[error("driver kind must not be empty")]
    Empty,
    #[error("driver kind is {length} bytes, exceeding the {max}-byte limit")]
    TooLong { length: usize, max: usize },
    #[error("driver kind contains unsupported character `{character}`")]
    InvalidCharacter { character: char },
}

#[derive(Debug, Error)]
#[non_exhaustive]
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

impl DriverError {
    pub fn backend(error: impl Error + Send + Sync + 'static) -> Self {
        Self::Backend(Box::new(error))
    }
}
