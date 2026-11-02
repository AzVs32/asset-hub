//! Backend-independent failures shared by instance opening and storage I/O.
use super::{InstanceId, StorageCapabilitiesError, StoragePathError};
use std::{error::Error, fmt};
use thiserror::Error;

/// Stable classifications, not an automatic retry policy. Timeouts, stale handles
/// and cancelled commits require operation-specific recovery and identity checks.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum StorageErrorKind {
    #[error("entry was not found")]
    NotFound,
    #[error("destination already exists")]
    AlreadyExists,
    #[error("permission denied")]
    PermissionDenied,
    #[error("entry is not a dir")]
    NotDir,
    #[error("entry is a dir")]
    IsDir,
    #[error("dir is not empty")]
    DirNotEmpty,
    #[error("invalid storage path")]
    InvalidPath,
    #[error("invalid storage request")]
    InvalidRequest,
    #[error("entry name cannot be represented without loss")]
    InvalidNameEncoding,
    #[error("stored instance data is invalid")]
    InvalidData,
    #[error("the storage instance root cannot be modified")]
    RootProtected,
    #[error("storage session is closed")]
    SessionClosed,
    #[error("storage operation is unsupported")]
    UnsupportedOperation,
    #[error("storage handle is stale")]
    StaleHandle,
    #[error("storage operation timed out")]
    TimedOut,
    #[error("storage backend is unavailable")]
    Unavailable,
    #[error("storage request was throttled")]
    RateLimited,
    #[error("storage precondition was not satisfied")]
    Conflict,
    #[error("storage capacity or quota was exceeded")]
    CapacityExceeded,
    #[error("operation crosses a storage device boundary")]
    CrossDevice,
    #[error("storage operation was cancelled")]
    Cancelled,
    #[error("operation outcome is unknown")]
    OutcomeUnknown,
    #[error("requested attribute was not found")]
    AttributeNotFound,
    #[error("attribute value or list exceeds the requested limit")]
    TooLarge,
    #[error("storage backend operation failed")]
    Backend,
}

/// A normalized category retaining its original internal diagnostic chain.
/// Display excludes backend messages, which may contain credentials or locations.
pub struct StorageError {
    kind: StorageErrorKind,
    source: Option<Box<dyn Error + Send + Sync>>,
}
impl StorageError {
    pub fn new(kind: StorageErrorKind) -> Self {
        Self { kind, source: None }
    }
    /// Normalize at the adapter boundary without discarding the original cause.
    pub fn with_source(kind: StorageErrorKind, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            kind,
            source: Some(Box::new(source)),
        }
    }
    pub fn kind(&self) -> StorageErrorKind {
        self.kind
    }
}
impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.kind, formatter)
    }
}
impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|source| source as _)
    }
}
impl From<StoragePathError> for StorageError {
    fn from(error: StoragePathError) -> Self {
        Self::with_source(StorageErrorKind::InvalidPath, error)
    }
}
impl From<StorageCapabilitiesError> for StorageError {
    fn from(error: StorageCapabilitiesError) -> Self {
        Self::with_source(StorageErrorKind::InvalidData, error)
    }
}
/// Structured creation-parameter failures without disclosing parameter values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceConfigErrorKind {
    MissingField,
    UnknownField,
    InvalidType,
    InvalidValue,
}

/// Driver operations distinguish malformed creation parameters from backend failures.
#[derive(Debug, Error)]
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

impl std::fmt::Debug for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageError")
            .field("kind", &self.kind)
            .field("has_source", &self.source.is_some())
            .finish()
    }
}
