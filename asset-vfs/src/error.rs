use crate::driver::error::DriverError;
use crate::mount::error::MountError;
use crate::namespace::error::NamespaceError;
use thiserror::Error;

/// The public error boundary for VFS operations.
///
/// Each variant groups errors from one domain. Domain errors are not exported
/// as standalone types; their details are
/// available through [`std::error::Error::source`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VfsError {
    #[error("namespace error: {0}")]
    Namespace(#[from] NamespaceError),
    #[error("driver error: {0}")]
    Driver(#[from] DriverError),
    #[error("mount error: {0}")]
    Mount(#[from] MountError),
}
