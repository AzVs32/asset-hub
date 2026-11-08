//! Entry metadata whose shape excludes file-size fields on non-files.
use super::StoragePath;
use getset::{CopyGetters, Getters};
use std::time::SystemTime;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EntryType {
    /// An adapter may omit a file's size when the backend cannot provide it.
    #[non_exhaustive]
    File {
        size: Option<u64>,
    },
    Directory,
    Symlink,
    Other,
}
impl EntryType {
    /// Construct file metadata; omitted size denotes an unknown size.
    pub fn file(size: Option<u64>) -> Self {
        Self::File { size }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum EntryError {
    #[error("the instance root must be a directory")]
    RootNotDirectory,
}
/// Metadata describes the selected entry. A lookup that preserves the final
/// symlink describes the link itself. The instance root is always a directory.
/// Native timestamp encodings are converted at the adapter boundary. Metadata
/// from separate operations need not describe the same backend revision.
#[derive(Clone, Debug, Eq, PartialEq, Getters, CopyGetters)]
pub struct Entry {
    /// The selected address. Use [`StoragePath::to_rooted_expr`] to reuse it
    /// as an operand in a new operation request.
    #[getset(get = "pub")]
    path: StoragePath,
    #[getset(get_copy = "pub")]
    entry_type: EntryType,
    #[getset(get_copy = "pub")]
    modified: Option<SystemTime>,
}

impl Entry {
    pub fn new(
        path: StoragePath,
        entry_type: EntryType,
        modified: Option<SystemTime>,
    ) -> Result<Self, EntryError> {
        if path.is_root() && entry_type != EntryType::Directory {
            return Err(EntryError::RootNotDirectory);
        }
        Ok(Self {
            path,
            entry_type,
            modified,
        })
    }
}
