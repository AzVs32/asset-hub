//! Directory query inputs and projections shared by application services and read-model adapters.

use crate::{
    CoreError,
    directory::domain::{Directory, DirectoryId, DirectoryPath},
};

/// A directory's stable identity and current path projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryLocation {
    id: DirectoryId,
    path: DirectoryPath,
}

impl DirectoryLocation {
    pub fn new(id: DirectoryId, path: DirectoryPath) -> Self {
        Self { id, path }
    }

    pub fn root() -> Self {
        Self::new(DirectoryId::root(), DirectoryPath::root())
    }

    pub fn id(&self) -> DirectoryId {
        self.id
    }

    pub fn path(&self) -> &DirectoryPath {
        &self.path
    }
}

/// A complete directory aggregate paired with its current path projection.
#[derive(Debug, Clone, PartialEq)]
pub struct LocatedDirectory {
    directory: Directory,
    location: DirectoryLocation,
}

impl LocatedDirectory {
    pub fn new(directory: Directory, location: DirectoryLocation) -> Result<Self, CoreError> {
        if directory.id() != location.id() {
            return Err(CoreError::invariant(
                "directory aggregate does not match its location projection",
            ));
        }
        Ok(Self {
            directory,
            location,
        })
    }

    pub fn directory(&self) -> &Directory {
        &self.directory
    }

    pub fn location(&self) -> &DirectoryLocation {
        &self.location
    }

    pub fn id(&self) -> DirectoryId {
        self.directory.id()
    }

    pub fn path(&self) -> &DirectoryPath {
        self.location.path()
    }

    pub fn into_directory(self) -> Directory {
        self.directory
    }

    pub fn into_parts(self) -> (Directory, DirectoryLocation) {
        (self.directory, self.location)
    }
}
