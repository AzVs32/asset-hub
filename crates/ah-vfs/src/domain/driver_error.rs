//! Driver lifecycle failures, separate from instance-local storage I/O.

use super::{InstanceId, storage::StorageError};
use thiserror::Error;

/// Structured creation-parameter failures without disclosing parameter values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InstanceConfigErrorKind {
    MissingField,
    UnknownField,
    InvalidType,
    InvalidValue,
}

/// Driver operations distinguish malformed creation parameters from backend failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DriverError {
    #[error("invalid instance configuration")]
    InvalidInstanceConfig {
        field: String,
        kind: InstanceConfigErrorKind,
    },
    #[error("driver instance '{0}' was not found")]
    InstanceNotFound(InstanceId),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

impl DriverError {
    /// Inspect a normalized storage failure. Configuration and missing-instance
    /// errors have no storage classification.
    pub fn storage_error(&self) -> Option<&StorageError> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}
