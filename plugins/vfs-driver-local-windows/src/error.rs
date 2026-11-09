//! Local instance configuration failures.

use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
use ah_vfs::domain::DriverKeyError;
use ah_vfs_storage::{Failure, Rejection};
use std::{io, path::PathBuf};
use thiserror::Error;

/// Classify native failures only at the adapter boundary; callers never inspect errno.
pub(crate) fn io_failure(error: &io::Error) -> Failure {
    match error.raw_os_error().map(|code| code as u32) {
        Some(32 | 33 | 303) => return Failure::Rejected(Rejection::Conflict),
        Some(17) => return Failure::Rejected(Rejection::CrossDevice),
        Some(50 | 4390 | 4392 | 4393 | 4394) => return Failure::Rejected(Rejection::Unsupported),
        Some(123 | 161) => return Failure::Rejected(Rejection::UnsupportedName),
        Some(206) => return Failure::Rejected(Rejection::NameTooLong),
        Some(145) => return Failure::Rejected(Rejection::DirectoryNotEmpty),
        _ => {}
    }
    ah_vfs::infrastructure::storage_io::io_failure(error)
}

impl From<WindowsLocalDriverError> for DriverError {
    fn from(error: WindowsLocalDriverError) -> Self {
        let failure = match &error {
            WindowsLocalDriverError::DriverKey(_) => LifecycleFailure::InvalidDeclaration,
            WindowsLocalDriverError::Dir { .. } => LifecycleFailure::Backend,
            _ => LifecycleFailure::InvalidRecord,
        };
        LifecycleError::with_source(failure, error).into()
    }
}

#[derive(Debug, Error)]
pub enum WindowsLocalDriverError {
    #[error("driver key declaration is invalid")]
    DriverKey(#[from] DriverKeyError),
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
