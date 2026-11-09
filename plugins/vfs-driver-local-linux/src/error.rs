//! Local instance configuration failures.

use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
use ah_vfs::domain::DriverKeyError;
use ah_vfs_storage::{BackendFailure, Failure, Rejection};
use std::{io, path::PathBuf};
use thiserror::Error;

/// Classify native failures only at the adapter boundary; callers never inspect errno.
pub(crate) fn io_failure(error: &io::Error) -> Failure {
    if let Some(code) = error.raw_os_error() {
        let code = rustix::io::Errno::from_raw_os_error(code);
        if code == rustix::io::Errno::NAMETOOLONG {
            return Failure::Rejected(Rejection::NameTooLong);
        }
        if code == rustix::io::Errno::STALE {
            return Failure::Backend(BackendFailure::Stale);
        }
        if code == rustix::io::Errno::NODATA {
            return Failure::Rejected(Rejection::AttributeNotFound);
        }
    }
    ah_vfs::infrastructure::storage_io::io_failure(error)
}

impl From<LinuxLocalDriverError> for DriverError {
    fn from(error: LinuxLocalDriverError) -> Self {
        let failure = match &error {
            LinuxLocalDriverError::DriverKey(_) => LifecycleFailure::InvalidDeclaration,
            LinuxLocalDriverError::Dir { .. } => LifecycleFailure::Backend,
            _ => LifecycleFailure::InvalidRecord,
        };
        LifecycleError::with_source(failure, error).into()
    }
}

#[derive(Debug, Error)]
pub enum LinuxLocalDriverError {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_native_errors_without_discarding_their_causes() {
        for (native, expected) in [
            (
                io::ErrorKind::NotFound,
                Failure::Rejected(Rejection::NotFound),
            ),
            (
                io::ErrorKind::PermissionDenied,
                Failure::Rejected(Rejection::AccessDenied),
            ),
            (
                io::ErrorKind::TimedOut,
                Failure::Backend(BackendFailure::TimedOut),
            ),
            (
                io::ErrorKind::CrossesDevices,
                Failure::Rejected(Rejection::CrossDevice),
            ),
            (
                io::ErrorKind::InvalidData,
                Failure::Backend(BackendFailure::CorruptData),
            ),
        ] {
            let source = io::Error::new(native, "private-location");
            assert_eq!(io_failure(&source), expected);
            let error = DriverError::from(LinuxLocalDriverError::Dir {
                path: PathBuf::from("/private-location"),
                source,
            });
            assert!(
                matches!(error, DriverError::Lifecycle(error) if error.failure() == LifecycleFailure::Backend && std::error::Error::source(&error).is_some() && !error.to_string().contains("private-location"))
            );
        }
    }
    #[test]
    fn stale_handles_and_missing_attributes_are_not_missing_entries() {
        let stale = io::Error::from_raw_os_error(rustix::io::Errno::STALE.raw_os_error());
        assert_eq!(io_failure(&stale), Failure::Backend(BackendFailure::Stale));
        let missing = io::Error::from_raw_os_error(rustix::io::Errno::NODATA.raw_os_error());
        assert_eq!(
            io_failure(&missing),
            Failure::Rejected(Rejection::AttributeNotFound)
        );
    }
}
