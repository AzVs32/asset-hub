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
    #[error("directory is not empty")]
    DirectoryNotEmpty,
    #[error("requested operation or guarantee is unsupported")]
    Unsupported,
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
    #[error("backend name cannot be represented losslessly")]
    UnrepresentableName,
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
    #[error("adapter interfaces disagree with its declaration")]
    CapabilityMismatch,
}

/// Why an operation failed. No variant contains backend text or caller data.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum Failure {
    #[error("{0}")]
    Rejected(Rejection),
    #[error("{0}")]
    Backend(BackendFailure),
    #[error("{0}")]
    Contract(ContractViolation),
}

/// Effects on content and explicitly managed attributes, not incidental access times.
/// Neither variant is an automatic retry instruction: identity and preconditions
/// may have changed even when the failed operation made no changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MutationOutcome {
    Unchanged,
    /// Includes partial effects. Reconcile the target before retrying a mutation.
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
