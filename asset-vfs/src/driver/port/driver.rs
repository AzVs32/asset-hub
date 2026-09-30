use crate::driver::{DriverKind, DriverPath, ReadDriver, WriteDriver};
use crate::error::VfsError;

/// A factory for backends rooted at driver-specific locations.
///
/// [`DriverPath`] is opaque to the VFS: each implementation defines its own
/// path syntax and resolves it when [`Driver::bind`] is called. A successful
/// bind fixes the backend's root, but does not promise a snapshot of the
/// contents beneath it. Driver errors are converted into [`VfsError`] with
/// `?` or `.into()`.
pub trait Driver: Send + Sync {
    /// Returns the stable identifier of this driver implementation.
    ///
    /// The kind is independent of bound roots and backend contents, and must
    /// remain the same across instances and calls. Mount definitions persist
    /// this identifier to select the corresponding driver implementation.
    fn kind(&self) -> DriverKind;

    /// Returns whether mounts using this driver allow descendant mounts.
    ///
    /// This is a fixed policy for the driver kind, independent of the bound
    /// root or backend contents, and can be queried without binding a backend.
    /// It describes this driver as a parent; it does not restrict where a mount
    /// using this driver may be placed. Mount management must enforce this
    /// policy.
    fn allows_submounts(&self) -> bool;

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
