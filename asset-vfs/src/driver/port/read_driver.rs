use crate::entry::Entry;
use crate::error::VfsError;
use crate::namespace::VirtualRelativePath;

/// Directory listing within one bound driver root.
///
/// All operation paths are canonical [`VirtualRelativePath`] values. The
/// empty path denotes the bound root. Implementations must keep operations
/// inside that root, including when native paths contain symbolic links.
/// Calls are independent observations: concurrent changes to the underlying
/// storage may affect a call or later calls.
pub trait ReadDriver: Send + Sync {
    /// Lists the direct children of `path`.
    ///
    /// Contract:
    /// - `path` is relative to the bound root; the empty path lists that root.
    /// - Return only immediate children, without duplicate names, in ascending
    ///   [`Entry::name`] order. Unsupported native entry types may be omitted.
    /// - Every returned name is a valid virtual entry name. When the combined
    ///   path fits the virtual path length limit, appending the name to `path`
    ///   addresses that child. Directory children can be listed provided the
    ///   underlying entry has not changed.
    /// - Report each supported child's actual file or directory kind.
    ///   Directories have no size. A file size may be absent; when present,
    ///   it is the file's byte length observed during listing.
    /// - Use [`crate::driver::DriverError::NotDirectory`] when `path` is a file and
    ///   [`crate::driver::DriverError::NotFound`] when the target is absent beneath existing
    ///   directories. If an ancestor is a file, either error may be used.
    /// - Use [`crate::driver::DriverError::UnrepresentableName`] if a supported native
    ///   child cannot be named in the virtual namespace.
    ///
    /// Storage or I/O failures may be wrapped with [`crate::driver::DriverError::backend`].
    fn list(&self, path: &VirtualRelativePath) -> Result<Vec<Entry>, VfsError>;
}
