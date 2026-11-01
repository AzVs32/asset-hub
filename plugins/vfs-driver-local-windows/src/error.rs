//! Local instance configuration failures.

use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind};
use std::{io, path::PathBuf};
use thiserror::Error;

/// Classify native failures only at the adapter boundary; callers never inspect errno.
pub(crate) fn io_error_kind(error: &io::Error) -> StorageErrorKind {
    match error.raw_os_error().map(|code| code as u32) {
        Some(32 | 33 | 303) => return StorageErrorKind::Conflict,
        Some(17) => return StorageErrorKind::CrossDevice,
        Some(50 | 4390 | 4392 | 4393 | 4394) => return StorageErrorKind::UnsupportedOperation,
        Some(123 | 161 | 206) => return StorageErrorKind::InvalidPath,
        Some(145) => return StorageErrorKind::DirNotEmpty,
        _ => {}
    }
    ah_vfs::infrastructure::sqlite_error::io_error_kind(error)
}

impl From<WindowsLocalDriverError> for DriverError {
    fn from(error: WindowsLocalDriverError) -> Self {
        let kind = match error {
            WindowsLocalDriverError::Dir { ref source, .. } => io_error_kind(source),
            WindowsLocalDriverError::NotDir(_) => StorageErrorKind::NotDir,
            WindowsLocalDriverError::InvalidPathEncoding => StorageErrorKind::InvalidNameEncoding,
            WindowsLocalDriverError::InvalidData => StorageErrorKind::InvalidData,
        };
        Self::Storage(StorageError::with_source(kind, error))
    }
}

#[derive(Debug, Error)]
pub enum WindowsLocalDriverError {
    #[error("local directory '{path}' is unavailable")]
    Dir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("local directory '{0}' is not a directory")]
    NotDir(PathBuf),
    #[error("local directory paths must be valid UTF-8")]
    InvalidPathEncoding,
    #[error("local instance configuration is invalid")]
    InvalidData,
}
