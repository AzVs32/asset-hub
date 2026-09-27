use crate::driver::{DriverPath, ReadDriver, WriteDriver};
use crate::error::VfsError;

/// A factory for backends rooted at driver-specific locations.
///
/// [`DriverPath`] is opaque to the VFS: each implementation defines its own
/// path syntax and resolves it when [`Driver::bind`] is called. A successful
/// bind fixes the backend's root, but does not promise a snapshot of the
/// contents beneath it. Driver errors are converted into [`VfsError`] with
/// `?` or `.into()`.
pub trait Driver: Send + Sync {
    /// Creates a backend bound to `root`.
    ///
    /// Contract:
    /// - Validate and resolve the driver-specific root before returning.
    /// - Use [`crate::driver::DriverError::InvalidPath`] for a path invalid under this
    ///   driver's syntax, [`crate::driver::DriverError::NotFound`] for a missing root, and
    ///   [`crate::driver::DriverError::NotDirectory`] for a root that is a file.
    /// - A successful backend interprets every operation relative to this
    ///   root and cannot access entries outside it.
    ///
    /// Other storage or I/O failures may be wrapped with [`crate::driver::DriverError::backend`].
    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, VfsError>;
}

/// A backend bound to one driver-specific root.
///
/// Reading is required; other interfaces may be optional. Every returned
/// interface operates within the same bound root, including when native paths
/// contain symbolic links. The interfaces borrow this backend and do not change
/// its root.
pub trait BoundDriver: Send + Sync {
    /// Returns the required read interface for this root.
    fn reader(&self) -> &dyn ReadDriver;

    /// Returns the optional write interface for this root.
    ///
    /// Backends without this interface return `None` by default.
    fn as_writer(&self) -> Option<&dyn WriteDriver> {
        None
    }
}

#[cfg(test)]
mod tests;
