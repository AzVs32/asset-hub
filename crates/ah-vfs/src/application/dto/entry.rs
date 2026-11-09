//! Requests and namespace views using absolute virtual paths.

use crate::domain::namespace::VirtualPath;
use crate::domain::storage::DEFAULT_DIR_PAGE_LIMIT;
use ah_vfs_storage::WriteMode;
use ah_vfs_storage::{Entry, EntryType};

use std::{num::NonZeroUsize, time::SystemTime};

/// Namespace entries can shadow backend entries at mount boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryOrigin {
    Storage,
    MountPoint { enabled: bool },
    SyntheticDir,
}

#[derive(Clone, Debug)]
pub struct EntryView {
    pub path: VirtualPath,
    pub kind: EntryType,
    pub modified: Option<SystemTime>,
    pub origin: EntryOrigin,
}

impl EntryView {
    pub(crate) fn from_storage(path: VirtualPath, entry: Entry) -> Self {
        Self {
            path,
            kind: entry.entry_type(),
            modified: entry.modified(),
            origin: EntryOrigin::Storage,
        }
    }

    pub(crate) fn dir(path: VirtualPath, origin: EntryOrigin) -> Self {
        Self {
            path,
            kind: EntryType::Directory,
            modified: None,
            origin,
        }
    }
}

/// Name-ordered live pagination after merging backend and namespace children.
pub struct DirQuery {
    pub after: Option<String>,
    pub limit: NonZeroUsize,
}

impl Default for DirQuery {
    fn default() -> Self {
        Self {
            after: None,
            limit: DEFAULT_DIR_PAGE_LIMIT,
        }
    }
}

pub struct DirPage {
    pub entries: Vec<EntryView>,
    /// Exclusive name cursor for the next page, not an absolute path.
    pub next_after: Option<String>,
}

pub struct ReadFileRequest {
    pub path: VirtualPath,
    pub offset: u64,
    pub length: Option<u64>,
}

pub struct WriteFileRequest {
    /// Require atomic complete-file publication; unsupported guarantees fail before staging.
    pub require_atomic_publish: bool,
    pub path: VirtualPath,
    pub mode: WriteMode,
}

pub struct CreateDirRequest {
    pub path: VirtualPath,
    pub recursive: bool,
}

pub struct RemoveEntryRequest {
    pub path: VirtualPath,
}

pub struct MoveEntryRequest {
    pub source: VirtualPath,
    pub destination: VirtualPath,
    pub replace: bool,
}
