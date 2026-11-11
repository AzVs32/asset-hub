//! Public failure semantics, independent of diagnostics and transport status codes.
use thiserror::Error;

/// A supported request could not be satisfied under its preconditions.
/// These reasons alone never establish whether a dispatched mutation had effects.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum Rejection {
    #[error("entry was not found")]
    NotFound,
    #[error("destination already exists")]
    AlreadyExists,
    #[error("access is denied")]
    AccessDenied,
    #[error("entry is not a directory")]
    NotDirectory,
    #[error("entry is a directory")]
    IsDirectory,
    #[error("entry is not a regular file")]
    NotRegularFile,
    #[error("entry is not a symbolic link")]
    NotSymlink,
    #[error("the instance root is protected")]
    RootProtected,
    #[error("symbolic links are not allowed by the lookup policy")]
    SymlinkNotAllowed,
    #[error("symbolic link traversal limit was exceeded")]
    TooManySymlinks,
    /// One logical entry component or attribute key exceeds the backend's
    /// native length limit, measured in the backend's own units.
    #[error("name exceeds the backend component or key length limit")]
    NameTooLong,
    /// The backend cannot address or store this valid logical spelling
    /// exactly, for example because it reserves the name or rewrites it.
    #[error("exact name is unsupported by the backend")]
    UnsupportedName,
    /// The logical address or operand exceeds its UTF-8 byte bound.
    #[error("path exceeds the logical address limit")]
    PathTooLong,
    #[error("directory is not empty")]
    DirectoryNotEmpty,
    #[error("requested operation or guarantee is unsupported")]
    Unsupported,
    /// Also used when an absent exact name cannot be created because a
    /// differently spelled, backend-equivalent name occupies the native slot.
    #[error("operation precondition was not satisfied")]
    Conflict,
    #[error("operation crosses a device boundary")]
    CrossDevice,
    #[error("attribute was not found")]
    AttributeNotFound,
    #[error("requested size limit would be exceeded")]
    LimitExceeded,
}

/// Failures of data, backend availability or the current resource lifetime.
/// Persisted instance records and driver configuration have their own application errors.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BackendFailure {
    #[error("backend is unavailable")]
    Unavailable,
    #[error("operation deadline expired")]
    TimedOut,
    #[error("operation was cancelled")]
    Cancelled,
    #[error("backend throttled the operation")]
    RateLimited,
    #[error("backend capacity or quota was exhausted")]
    CapacityExceeded,
    #[error("stored entry data is corrupt")]
    CorruptData,
    /// An authoritative native directory entry name or attribute key is outside
    /// the logical namespace and cannot be returned as a logical name.
    #[error("backend name cannot be represented losslessly")]
    UnrepresentableName,
    #[error("symbolic link target cannot be represented as a lookup operand")]
    UnrepresentableLinkTarget,
    #[error("resource is closed")]
    Closed,
    #[error("resource belongs to a stale generation")]
    Stale,
    #[error("backend operation failed")]
    Other,
}

/// Bugs at an adapter boundary, separate from invalid caller input or stored data.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum ContractViolation {
    #[error("adapter declaration is malformed")]
    InvalidDeclaration,
    #[error("adapter response is malformed or unexpected")]
    InvalidResponse,
    #[error("adapter returned invalid entry metadata")]
    InvalidEntry,
    #[error("adapter returned an invalid directory page")]
    InvalidPage,
    #[error("adapter returned invalid attributes")]
    InvalidAttributes,
    #[error("adapter violated the byte-stream contract")]
    InvalidStream,
    /// The adapter failed to uphold a declared capability, such as atomic
    /// visibility during a write publication.
    #[error("adapter interfaces disagree with its declaration")]
    CapabilityMismatch,
}

/// Why an operation failed. No variant contains backend text or caller data.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum Failure {
    /// The caller supplied a request that disagrees with its binding.
    #[error("request does not match its backend binding")]
    InvalidBinding,
    #[error("{0}")]
    Rejected(Rejection),
    #[error("{0}")]
    Backend(BackendFailure),
    #[error("{0}")]
    Contract(ContractViolation),
}

/// The failed attempt's effects on visible directory entries, published
/// contents and explicitly managed attributes, including permissions and
/// extended attributes. Incidental access times and other automatic
/// bookkeeping, private staging and cleanup of private resources are outside
/// this scope. Effects on visible parents count as well as effects on the
/// final target.
///
/// This is independent of the cause and carries no retry authorization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationOutcome {
    /// The entire attempt is confirmed to have made no covered changes and
    /// cannot make any later. A visible change followed by cleanup does not
    /// qualify.
    Unchanged,
    /// Covered effects are possible or known to have occurred. This includes
    /// confirmed partial changes and publication followed by a failure to
    /// complete the operation. Reconcile before any unprotected replay.
    Unknown,
}
impl std::fmt::Display for MutationOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unchanged => "no mutation effects",
            Self::Unknown => "mutation effects require reconciliation",
        })
    }
}
