//! Backend-independent storage I/O failures and diagnostic chains.
use super::{EntryPermissionsError, StorageCapabilitiesError, StoragePathError, XattrNameError};
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
    /// Stored records or backend data are corrupt or cannot be interpreted.
    #[error("stored or backend data is invalid")]
    InvalidData,
    /// An adapter declaration, implementation or response violates the storage contract.
    #[error("storage adapter violated its contract")]
    ProtocolViolation,
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
///
/// `From` conversions for storage value validation errors assume request input:
/// paths become `InvalidPath`; permissions and attribute names become `InvalidRequest`.
/// Response decoders and persistence adapters must use `with_source` with their
/// context's category instead. Capability validation is an adapter declaration
/// failure, so `From<StorageCapabilitiesError>` yields `ProtocolViolation`.
/// Validation errors themselves remain independent of these classifications.
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
impl From<EntryPermissionsError> for StorageError {
    fn from(error: EntryPermissionsError) -> Self {
        Self::with_source(StorageErrorKind::InvalidRequest, error)
    }
}
impl From<XattrNameError> for StorageError {
    fn from(error: XattrNameError) -> Self {
        Self::with_source(StorageErrorKind::InvalidRequest, error)
    }
}
impl From<StorageCapabilitiesError> for StorageError {
    fn from(error: StorageCapabilitiesError) -> Self {
        Self::with_source(StorageErrorKind::ProtocolViolation, error)
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
