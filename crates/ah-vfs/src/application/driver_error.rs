//! Driver lifecycle failures, separate from instance-local storage I/O.

use crate::domain::InstanceId;
use ah_vfs_storage::StorageError;
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
    Lifecycle(#[from] LifecycleError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

impl DriverError {
    /// Inspect an instance or driver lifecycle failure.
    pub fn lifecycle_error(&self) -> Option<&LifecycleError> {
        match self {
            Self::Lifecycle(error) => Some(error),
            _ => None,
        }
    }

    /// Inspect a normalized storage failure. Configuration and missing-instance
    /// errors have no storage classification.
    pub fn storage_error(&self) -> Option<&StorageError> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

/// Failures before opening a storage instance or in the host's instance records.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum LifecycleFailure {
    #[error("instance access denied")]
    AccessDenied,
    #[error("instance generation is stale")]
    Stale,
    #[error("instance resource capacity exceeded")]
    CapacityExceeded,
    #[error("instance lifecycle operation cancelled")]
    Cancelled,
    #[error("invalid stored instance record")]
    InvalidRecord,
    #[error("invalid lifecycle request")]
    InvalidRequest,
    #[error("invalid driver declaration")]
    InvalidDeclaration,
    #[error("requested lifecycle operation is unsupported")]
    Unsupported,
    #[error("instance record changed concurrently")]
    Conflict,
    #[error("instance backend is unavailable")]
    Unavailable,
    #[error("instance backend deadline expired")]
    TimedOut,
    #[error("instance lifecycle backend failed")]
    Backend,
}

#[derive(Error)]
#[error("{failure}")]
pub struct LifecycleError {
    failure: LifecycleFailure,
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl LifecycleError {
    pub fn with_boxed_source(
        failure: LifecycleFailure,
        source: Box<dyn std::error::Error + Send + Sync>,
    ) -> Self {
        Self {
            failure,
            source: Some(source),
        }
    }
    pub fn new(failure: LifecycleFailure) -> Self {
        Self {
            failure,
            source: None,
        }
    }

    pub fn with_source(
        failure: LifecycleFailure,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            failure,
            source: Some(Box::new(source)),
        }
    }

    pub fn failure(&self) -> LifecycleFailure {
        self.failure
    }
}

impl std::fmt::Debug for LifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LifecycleError")
            .field("failure", &self.failure)
            .finish()
    }
}
