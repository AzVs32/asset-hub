//! Requests and namespace views using absolute virtual paths.

use crate::domain::namespace::{MountTable, VirtualPath, VirtualPathError};
use ah_vfs_storage::WriteMode;
use ah_vfs_storage::{DirectoryCursor, DirectorySelection, Entry, EntryType, PageSize};

use std::{sync::Arc, time::SystemTime};

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
#[derive(Default)]
pub struct DirQuery {
    pub cursor: Option<VfsDirectoryCursor>,
    pub size: PageSize,
}

/// A virtual directory checkpoint. Its backend checkpoint, if present, belongs
/// to the bound instance and is never used as a virtual namespace address.
#[derive(Clone)]
pub struct VfsDirectoryCursor {
    pub(crate) path: VirtualPath,
    pub(crate) mounts: Arc<MountTable>,
    pub(crate) storage: Arc<dyn crate::application::port::Storage>,
    pub(crate) after: String,
    pub(crate) directory: Option<DirectorySelection>,
    pub(crate) backend: Option<DirectoryCursor>,
}

pub struct DirPage {
    pub entries: Vec<EntryView>,
    pub continuation: Option<VfsDirectoryCursor>,
}

pub struct ReadFileRequest {
    pub path: VirtualPath,
    pub range: ah_vfs_storage::ReadRange,
}

pub struct WriteFileRequest {
    /// Require atomic complete-file publication; unsupported guarantees fail before staging.
    pub publication: ah_vfs_storage::Publication,
    pub path: VirtualPath,
    pub mode: WriteMode,
}

/// An absolute VFS operand. Routing uses the canonical address and retains
/// the caller's trailing slash as an instance-local directory requirement.
pub struct CreateDirRequest {
    pub(crate) path: VirtualPath,
    pub(crate) mode: crate::resolve::operand::CreateDirectoryMode,
    pub(crate) trailing_slash: bool,
}

impl CreateDirRequest {
    pub fn new(
        path: &str,
        mode: crate::resolve::operand::CreateDirectoryMode,
    ) -> Result<Self, VirtualPathError> {
        Ok(Self {
            path: VirtualPath::parse(path)?,
            mode,
            trailing_slash: path.ends_with('/'),
        })
    }
}

/// An instance-local removal routed by an absolute VFS path.
pub struct RemoveEntryRequest {
    pub(crate) path: VirtualPath,
    pub(crate) kind: ah_vfs_storage::RemoveKind,
    pub(crate) trailing_slash: bool,
}

impl RemoveEntryRequest {
    pub fn new(path: &str, kind: ah_vfs_storage::RemoveKind) -> Result<Self, VirtualPathError> {
        Ok(Self {
            path: VirtualPath::parse(path)?,
            kind,
            trailing_slash: path.ends_with('/'),
        })
    }
}

/// A rename routed within a single VFS mount.
pub struct MoveEntryRequest {
    pub(crate) source: VirtualPath,
    pub(crate) destination: VirtualPath,
    pub(crate) mode: ah_vfs_storage::RenameMode,
    pub(crate) source_slash: bool,
    pub(crate) destination_slash: bool,
}

impl MoveEntryRequest {
    pub fn new(
        source: &str,
        destination: &str,
        mode: ah_vfs_storage::RenameMode,
    ) -> Result<Self, VirtualPathError> {
        Ok(Self {
            source: VirtualPath::parse(source)?,
            destination: VirtualPath::parse(destination)?,
            mode,
            source_slash: source.ends_with('/'),
            destination_slash: destination.ends_with('/'),
        })
    }
}
