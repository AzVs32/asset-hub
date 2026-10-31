use ah_vfs::domain::{DriverError, StorageErrorKind};
use std::{error::Error, io};

#[test]
fn sql_failures_are_normalized_without_losing_sources_or_disclosing_messages() {
    for (source, expected) in [
        (sqlx::Error::PoolClosed, StorageErrorKind::Unavailable),
        (sqlx::Error::PoolTimedOut, StorageErrorKind::TimedOut),
        (
            sqlx::Error::Io(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private-token",
            )),
            StorageErrorKind::PermissionDenied,
        ),
        (
            sqlx::Error::Io(io::Error::new(io::ErrorKind::NotFound, "private-token")),
            StorageErrorKind::NotFound,
        ),
        (
            sqlx::Error::Io(io::Error::new(io::ErrorKind::TimedOut, "private-token")),
            StorageErrorKind::TimedOut,
        ),
        (sqlx::Error::RowNotFound, StorageErrorKind::Backend),
    ] {
        let failure = vfs_driver_kit::database_error(source);
        assert!(!failure.to_string().contains("private-token"));
        assert!(
            matches!(failure, DriverError::Storage(error) if error.kind() == expected && error.source().is_some())
        );
    }
}
