//! Normalized SQLite adapter failures shared by record stores.
use crate::domain::{
    DriverError,
    storage::{StorageError, StorageErrorKind},
};
use std::io;
/// Portable I/O categories. Native filesystem-specific errno refinements remain
/// in adapters, before this common fallback.
pub fn io_error_kind(source: &io::Error) -> StorageErrorKind {
    use StorageErrorKind as S;
    use io::ErrorKind as I;
    match source.kind() {
        I::NotFound => S::NotFound,
        I::AlreadyExists => S::AlreadyExists,
        I::PermissionDenied => S::PermissionDenied,
        I::NotADirectory => S::NotDir,
        I::IsADirectory => S::IsDir,
        I::DirectoryNotEmpty => S::DirNotEmpty,
        I::InvalidInput => S::InvalidRequest,
        I::InvalidData => S::InvalidData,
        I::Unsupported => S::UnsupportedOperation,
        I::TimedOut => S::TimedOut,
        I::ConnectionRefused
        | I::ConnectionReset
        | I::ConnectionAborted
        | I::NotConnected
        | I::NetworkUnreachable
        | I::HostUnreachable => S::Unavailable,
        I::StorageFull | I::QuotaExceeded => S::CapacityExceeded,
        I::CrossesDevices => S::CrossDevice,
        _ => S::Backend,
    }
}

/// Preserve the original SQL error behind a normalized, credential-safe Display.
pub fn database_error(source: sqlx::Error) -> DriverError {
    let kind = match &source {
        sqlx::Error::PoolClosed => StorageErrorKind::Unavailable,
        sqlx::Error::PoolTimedOut => StorageErrorKind::TimedOut,
        sqlx::Error::Io(error) => io_error_kind(error),
        _ => StorageErrorKind::Backend,
    };
    StorageError::with_source(kind, source).into()
}
