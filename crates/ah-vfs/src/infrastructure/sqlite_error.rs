//! Normalized SQLite adapter failures shared by record stores.
use crate::application::driver_error::DriverError;
use std::io;
/// Preserve the original SQL error behind a normalized, credential-safe Display.
pub fn database_error(source: sqlx::Error) -> DriverError {
    use crate::application::driver_error::{LifecycleError, LifecycleFailure as L};
    let failure = match &source {
        sqlx::Error::PoolClosed => L::Unavailable,
        sqlx::Error::PoolTimedOut => L::TimedOut,
        sqlx::Error::ColumnDecode { .. } => L::InvalidRecord,
        sqlx::Error::Io(error) => match error.kind() {
            io::ErrorKind::TimedOut => L::TimedOut,
            io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable => L::Unavailable,
            _ => L::Backend,
        },
        _ => L::Backend,
    };
    LifecycleError::with_source(failure, source).into()
}
