use std::io::Read;

use crate::domain::Entry;
use crate::domain::{DriverPath, VirtualRelativePath};
use crate::error::CoreError;

/// A factory for read-only backends rooted at driver-specific locations.
///
/// [`DriverPath`] is opaque to the core: each implementation defines its own
/// path syntax and resolves it when [`Driver::bind`] is called. A successful
/// bind fixes the backend's root, but does not promise a snapshot of the
/// contents beneath it.
pub trait Driver: Send + Sync {
    /// Creates a backend bound to `root`.
    ///
    /// Contract:
    /// - Validate and resolve the driver-specific root before returning.
    /// - Use [`CoreError::driver_invalid_path`] for a path invalid under this
    ///   driver's syntax, [`CoreError::driver_not_found`] for a missing root, and
    ///   [`CoreError::driver_not_directory`] for a root that is a file.
    /// - A successful backend interprets every operation relative to this
    ///   root and cannot access entries outside it.
    ///
    /// Other storage or I/O failures may be wrapped with [`CoreError::backend`].
    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, CoreError>;
}

/// A read-only backend bound to one driver-specific root.
///
/// All operation paths are canonical [`VirtualRelativePath`] values. The
/// empty path denotes the bound root. Implementations must keep operations
/// inside that root, including when native paths contain symbolic links.
/// Calls are independent observations: concurrent changes to the underlying
/// storage may affect a call, later calls, or an open stream.
pub trait BoundDriver: Send + Sync {
    /// Lists the direct children of `path`.
    ///
    /// Contract:
    /// - `path` is relative to the bound root; the empty path lists that root.
    /// - Return only immediate children, without duplicate names, in ascending
    ///   [`Entry::name`] order. Unsupported native entry types may be omitted.
    /// - Every returned name is a valid virtual entry name. When the combined
    ///   path fits the virtual path length limit, appending the name to `path`
    ///   addresses that child for a subsequent `list` or `read`, provided the
    ///   underlying entry has not changed.
    /// - Report each supported child's actual file or directory kind.
    ///   Directories have no size. A file size may be absent; when present,
    ///   it is the file's byte length observed during listing, not a promise
    ///   about a later read after the file changes.
    /// - Use [`CoreError::driver_not_directory`] when `path` is a file and
    ///   [`CoreError::driver_not_found`] when the target is absent beneath existing
    ///   directories. If an ancestor is a file, either error may be used.
    /// - Use [`CoreError::driver_unrepresentable_name`] if a supported native
    ///   child cannot be named in the virtual namespace.
    ///
    /// Storage or I/O failures may be wrapped with [`CoreError::backend`].
    fn list(&self, path: &VirtualRelativePath) -> Result<Vec<Entry>, CoreError>;

    /// Opens a file at `path` for streaming reads.
    ///
    /// Contract:
    /// - `path` is relative to the bound root. A successful result is a
    ///   readable byte stream for the addressed file; the stream can outlive
    ///   the backend.
    /// - Use [`CoreError::driver_is_directory`] when `path` is a directory,
    ///   including the empty path, and [`CoreError::driver_not_found`] when the
    ///   target is absent beneath existing directories. If an ancestor is a
    ///   file, either [`CoreError::driver_not_found`] or
    ///   [`CoreError::driver_not_directory`] may be used.
    /// - Never follow a path outside the bound root. Unsupported native entry
    ///   types and other storage or I/O failures may be wrapped with
    ///   [`CoreError::backend`].
    ///
    /// The stream need not be a snapshot if the underlying file changes while
    /// it is read. Errors during streaming are reported as [`std::io::Error`].
    fn read(&self, path: &VirtualRelativePath)
    -> Result<Box<dyn Read + Send + 'static>, CoreError>;
}

#[cfg(test)]
mod tests;
