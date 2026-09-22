use crate::path::domain::EntryName;

/// The kind of object represented by an [`Entry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryKind {
    File,
    Directory,
}

/// One named object in the virtual namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entry {
    name: EntryName,
    kind: EntryKind,
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

    pub fn name(&self) -> &EntryName {
        &self.name
    }

    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    pub fn size(&self) -> Option<u64> {
        self.size
    }
}

#[cfg(test)]
mod tests;
