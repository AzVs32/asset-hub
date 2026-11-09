//! Native storage I/O classification, independent of persistence errors.
use std::io;
/// Portable I/O categories. Native filesystem-specific errno refinements remain
/// in adapters, before this common fallback.
pub fn io_failure(source: &io::Error) -> ah_vfs_storage::Failure {
    use ah_vfs_storage::{BackendFailure, Failure, Rejection};
    use io::ErrorKind as I;
    match source.kind() {
        I::NotFound => Failure::Rejected(Rejection::NotFound),
        I::AlreadyExists => Failure::Rejected(Rejection::AlreadyExists),
        I::PermissionDenied => Failure::Rejected(Rejection::AccessDenied),
        I::NotADirectory => Failure::Rejected(Rejection::NotDirectory),
        I::IsADirectory => Failure::Rejected(Rejection::IsDirectory),
        I::DirectoryNotEmpty => Failure::Rejected(Rejection::DirectoryNotEmpty),
        I::InvalidInput => Failure::Rejected(Rejection::Conflict),
        I::InvalidData => Failure::Backend(BackendFailure::CorruptData),
        I::Unsupported => Failure::Rejected(Rejection::Unsupported),
        I::TimedOut => Failure::Backend(BackendFailure::TimedOut),
        I::ConnectionRefused
        | I::ConnectionReset
        | I::ConnectionAborted
        | I::NotConnected
        | I::NetworkUnreachable
        | I::HostUnreachable => Failure::Backend(BackendFailure::Unavailable),
        I::StorageFull | I::QuotaExceeded => Failure::Backend(BackendFailure::CapacityExceeded),
        I::CrossesDevices => Failure::Rejected(Rejection::CrossDevice),
        _ => Failure::Backend(BackendFailure::Other),
    }
}
