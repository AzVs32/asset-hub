use crate::driver::DriverError;
use crate::mount::MountError;
use crate::namespace::NamespaceError;

/// The public error boundary for VFS operations.
///
/// Each variant groups errors from one domain. Domain error types are exported
/// from their domain entry points for construction and matching.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VfsError {
    #[error("namespace error: {0}")]
    Namespace(#[from] NamespaceError),
    #[error("driver error: {0}")]
    Driver(#[from] DriverError),
    #[error("mount error: {0}")]
    Mount(#[from] MountError),
}
