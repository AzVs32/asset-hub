//! Backend entry metadata using instance-local addresses.

use super::StoragePath;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageEntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// Metadata of an entry without following its final symbolic link.
#[derive(Clone, Debug)]
pub struct StorageEntry {
    pub path: StoragePath,
    pub kind: StorageEntryKind,
    /// Byte length for regular files; absent for other entry kinds.
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}
