use crate::namespace::EntryName;
use getset::{CopyGetters, Getters};

/// The kind of object represented by an [`Entry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryKind {
    File,
    Directory,
}

/// One named object in the virtual namespace.
///
/// Directories have no size. A file's size may be unknown to the driver.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Getters, CopyGetters)]
pub struct Entry {
    #[getset(get = "pub")]
    name: EntryName,
    #[getset(get_copy = "pub")]
    kind: EntryKind,
    #[getset(get_copy = "pub")]
    size: Option<u64>,
}

impl Entry {
    /// Creates a file entry. Its size may be unknown to the driver.
    pub fn file(name: EntryName, size: Option<u64>) -> Self {
        Self {
            name,
            kind: EntryKind::File,
            size,
        }
    }

    /// Creates a directory entry, which has no file size.
    pub fn directory(name: EntryName) -> Self {
        Self {
            name,
            kind: EntryKind::Directory,
            size: None,
        }
    }
}

#[cfg(test)]
mod tests;
