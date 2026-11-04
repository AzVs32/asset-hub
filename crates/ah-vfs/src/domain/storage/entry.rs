//! Backend entry metadata using instance-local addresses.

use super::StoragePath;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// Metadata of an entry without following its final symbolic link.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: StoragePath,
    pub kind: EntryKind,
    /// Byte length for regular files; absent for other entry kinds.
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}
