use crate::namespace::domain::EntryName;

/// The kind of object represented by an [`Entry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryKind {
    File,
    Directory,
}

/// Metadata describing an object at a virtual path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Metadata {
    kind: EntryKind,
    size: Option<u64>,
}

impl Metadata {
    /// Creates file metadata. Its size may be unknown to the driver.
    pub fn file(size: Option<u64>) -> Self {
        Self {
            kind: EntryKind::File,
            size,
        }
    }

    /// Creates directory metadata, which has no file size.
    pub fn directory() -> Self {
        Self {
            kind: EntryKind::Directory,
            size: None,
        }
    }

    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    pub fn size(&self) -> Option<u64> {
        self.size
    }
}

/// One named object in the virtual namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entry {
    name: EntryName,
    metadata: Metadata,
}

impl Entry {
    /// Creates a file entry. Its size may be unknown to the driver.
    pub fn file(name: EntryName, size: Option<u64>) -> Self {
        Self {
            name,
            metadata: Metadata::file(size),
        }
    }

    /// Creates a directory entry, which has no file size.
    pub fn directory(name: EntryName) -> Self {
        Self {
            name,
            metadata: Metadata::directory(),
        }
    }

    pub fn name(&self) -> &EntryName {
        &self.name
    }

    pub fn kind(&self) -> EntryKind {
        self.metadata.kind()
    }

    pub fn size(&self) -> Option<u64> {
        self.metadata.size()
    }

    pub fn metadata(&self) -> Metadata {
        self.metadata
    }
}

#[cfg(test)]
mod tests;
