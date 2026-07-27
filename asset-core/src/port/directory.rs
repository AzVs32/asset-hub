//! Directory aggregate persistence, query projections, and rebuildable index ports.

use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryPath},
};
use chrono::{DateTime, Utc};

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
            return Err(CoreError::configuration(
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

/// Durable storage for directory aggregates. Paths and tree projections are not persisted here.
#[async_trait::async_trait]
pub trait DirectoryStore: Send + Sync {
    async fn load_all(&self) -> Result<Vec<Directory>, CoreError>;
    async fn insert(&self, directory: &Directory) -> Result<(), CoreError>;
    async fn save_if_unchanged(
        &self,
        directory: &Directory,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError>;
    async fn remove_if_empty(&self, id: &DirectoryId) -> Result<bool, CoreError>;
}

/// Read-only directory tree projections.
#[async_trait::async_trait]
pub trait DirectoryQuery: Send + Sync {
    async fn find_by_id(&self, id: &DirectoryId) -> Result<Option<LocatedDirectory>, CoreError>;
    async fn find_by_path(
        &self,
        path: &DirectoryPath,
    ) -> Result<Option<LocatedDirectory>, CoreError>;
    async fn list_children(
        &self,
        parent_id: &DirectoryId,
    ) -> Result<Vec<LocatedDirectory>, CoreError>;
    async fn is_descendant_or_self(
        &self,
        ancestor_id: &DirectoryId,
        candidate_id: &DirectoryId,
    ) -> Result<bool, CoreError>;
}

/// A rebuildable directory query index kept in sync after durable writes commit.
#[async_trait::async_trait]
pub trait DirectoryIndex: DirectoryQuery {
    async fn replace_all(&self, directories: Vec<Directory>) -> Result<(), CoreError>;
    async fn upsert(&self, directory: Directory) -> Result<(), CoreError>;
    async fn remove(&self, id: &DirectoryId) -> Result<(), CoreError>;
}
