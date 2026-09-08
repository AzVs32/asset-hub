//! Resource query inputs and projections shared by application services and read-model adapters.

use crate::{
    CoreError,
    directory::{domain::DirectoryId, query::DirectoryLocation},
    resource::domain::Resource,
    storage::StorageKey,
};

/// Resource read-model filters. Directory identity is always a stable UUID; callers resolve
/// global paths through `DirectoryService` before constructing this value.
#[derive(Debug, Clone)]
pub struct ListResources {
    limit: u32,
    offset: u64,
    q: Option<String>,
    directory_id: DirectoryId,
}

impl ListResources {
    pub fn new(limit: u32, offset: u64, directory_id: DirectoryId) -> Self {
        Self {
            limit,
            offset,
            q: None,
            directory_id,
        }
    }

    pub fn with_q(mut self, q: impl Into<String>) -> Self {
        self.q = Some(q.into());
        self
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    pub fn directory_id(&self) -> DirectoryId {
        self.directory_id
    }
}

/// A Resource aggregate paired with its current Directory projection.
#[derive(Debug, Clone)]
pub struct LocatedResource {
    resource: Resource,
    directory: DirectoryLocation,
}

impl LocatedResource {
    pub fn new(resource: Resource, directory: DirectoryLocation) -> Result<Self, CoreError> {
        if resource.directory_id() != directory.id() {
            return Err(CoreError::invariant(
                "resource directory does not match its location projection",
            ));
        }
        Ok(Self {
            resource,
            directory,
        })
    }

    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    pub fn directory(&self) -> &DirectoryLocation {
        &self.directory
    }

    pub fn storage_key(&self) -> Result<StorageKey, CoreError> {
        super::storage_key_from_resource_path(self.directory.path(), self.resource.name())
            .map_err(Into::into)
    }

    pub fn into_resource(self) -> Resource {
        self.resource
    }

    pub fn into_parts(self) -> (Resource, DirectoryLocation) {
        (self.resource, self.directory)
    }
}

#[derive(Debug, Clone)]
pub struct ResourcePage {
    pub items: Vec<LocatedResource>,
    pub total: u64,
    pub limit: u32,
    pub offset: u64,
}
