//! Local instance configuration failures.

use ah_vfs::domain::{
    DriverError, DriverKeyError,
    storage::{StorageCapabilitiesError, StorageError, StorageErrorKind},
};
use std::{io, path::PathBuf};
use thiserror::Error;

/// Classify native failures only at the adapter boundary; callers never inspect errno.
pub(crate) fn io_error_kind(error: &io::Error) -> StorageErrorKind {
    if let Some(code) = error.raw_os_error() {
        let code = rustix::io::Errno::from_raw_os_error(code);
        if code == rustix::io::Errno::STALE {
            return StorageErrorKind::StaleHandle;
        }
        if code == rustix::io::Errno::NODATA {
            return StorageErrorKind::AttributeNotFound;
        }
    }
    ah_vfs::infrastructure::sqlite_error::io_error_kind(error)
}

impl From<LinuxLocalDriverError> for DriverError {
    fn from(error: LinuxLocalDriverError) -> Self {
        let kind = match error {
            LinuxLocalDriverError::Dir { ref source, .. } => io_error_kind(source),
            LinuxLocalDriverError::NotDir(_) => StorageErrorKind::NotDir,
            LinuxLocalDriverError::InvalidPathEncoding => StorageErrorKind::InvalidNameEncoding,
            LinuxLocalDriverError::InvalidData => StorageErrorKind::InvalidData,
            LinuxLocalDriverError::DriverKey(_) | LinuxLocalDriverError::Capabilities(_) => {
                StorageErrorKind::ProtocolViolation
            }
        };
        Self::Storage(StorageError::with_source(kind, error))
    }
}

#[derive(Debug, Error)]
pub enum LinuxLocalDriverError {
    #[error("driver key declaration is invalid")]
    DriverKey(#[from] DriverKeyError),
    #[error("storage capability declaration is invalid")]
    Capabilities(#[from] StorageCapabilitiesError),
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
            (io::ErrorKind::NotFound, StorageErrorKind::NotFound),
            (
                io::ErrorKind::PermissionDenied,
                StorageErrorKind::PermissionDenied,
            ),
            (io::ErrorKind::TimedOut, StorageErrorKind::TimedOut),
            (io::ErrorKind::CrossesDevices, StorageErrorKind::CrossDevice),
            (io::ErrorKind::InvalidData, StorageErrorKind::InvalidData),
        ] {
            let source = io::Error::new(native, "private-location");
            assert_eq!(io_error_kind(&source), expected);
            let error = DriverError::from(LinuxLocalDriverError::Dir {
                path: PathBuf::from("/private-location"),
                source,
            });
            assert!(
                matches!(error, DriverError::Storage(error) if error.kind() == expected && std::error::Error::source(&error).is_some() && !error.to_string().contains("private-location"))
            );
        }
    }
    #[test]
    fn stale_handles_and_missing_attributes_are_not_missing_entries() {
        let stale = io::Error::from_raw_os_error(rustix::io::Errno::STALE.raw_os_error());
        assert_eq!(io_error_kind(&stale), StorageErrorKind::StaleHandle);
        let missing = io::Error::from_raw_os_error(rustix::io::Errno::NODATA.raw_os_error());
        assert_eq!(io_error_kind(&missing), StorageErrorKind::AttributeNotFound);
    }
}
