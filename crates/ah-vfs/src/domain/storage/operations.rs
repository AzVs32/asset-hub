//! Instance-local operation requests.

use ah_vfs_storage::StoragePath;

/// Create a backend directory without overwriting file contents.
/// Recursive requests accept existing directories, including the instance root;
/// an existing file at the target or in its parent chain returns NotDir.
/// Non-recursive requests require existing parents and an absent target;
/// existing targets, including the instance root, return AlreadyExists.
pub struct StorageCreateDirRequest {
    pub path: StoragePath,
    /// Create missing parents and accept existing directories when true.
    pub recursive: bool,
}
/// Remove a file, symbolic link, or empty directory. The root is protected.
pub struct StorageRemoveEntryRequest {
    pub path: StoragePath,
}

/// Rename within an instance. Both roots are protected; parents must exist.
/// For supported requests with identical non-root paths, a missing source returns
/// NotFound. An existing source returns AlreadyExists when replace is false,
/// or succeeds without modification when replace is true.
pub struct StorageMoveEntryRequest {
    pub source: StoragePath,
    pub destination: StoragePath,
    /// Atomically replace a compatible destination entry when true.
    /// False requires an absent destination and atomic no-replace rename,
    /// which may be unsupported by a backend.
    pub replace: bool,
}
