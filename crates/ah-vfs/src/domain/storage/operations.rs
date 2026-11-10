//! Instance-local operation requests and directory query results.

use super::{DEFAULT_DIR_PAGE_LIMIT, validate_dir_page_limit};
use ah_vfs_storage::{Entry, EntryName, StoragePath};
use ah_vfs_storage::{Failure, Operation, Rejection, StorageError};
use std::num::NonZeroUsize;

/// Immediate children in ascending UTF-8 name order.
///
/// `after` is the previous page's last name, exclusively. This is a live listing,
/// not a snapshot; concurrent changes can affect later pages.
pub struct StorageDirQuery {
    pub path: StoragePath,
    pub after: Option<EntryName>,
    /// Maximum entries per page, bounded by [`super::MAX_DIR_PAGE_LIMIT`].
    pub limit: NonZeroUsize,
}
impl StorageDirQuery {
    pub fn new(path: StoragePath) -> Self {
        Self {
            path,
            after: None,
            limit: DEFAULT_DIR_PAGE_LIMIT,
        }
    }

    /// Validate the page limit and exclusive child-name cursor before backend I/O.
    /// Oversized pages return LimitExceeded; a joined address exceeding its bound
    /// returns PathTooLong with its original validation error as the source.
    pub fn validate(&self) -> Result<(), StorageError> {
        validate_dir_page_limit(self.limit)?;
        if let Some(after) = &self.after {
            self.path.join(after).map_err(|cause| {
                StorageError::without_effects(
                    Operation::List,
                    Failure::Rejected(Rejection::PathTooLong),
                    Some(Box::new(cause)),
                )
            })?;
        }
        Ok(())
    }
}
pub struct StorageDirPage {
    pub entries: Vec<Entry>,
    /// Pass as `after`; `None` means there are no more entries now.
    pub next_after: Option<EntryName>,
}

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
