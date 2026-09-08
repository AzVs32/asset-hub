//! Resource aggregate persistence, read-model, and relocation ports.

use crate::CoreError;
use crate::{
    directory::{domain::DirectoryId, port::DirectoryLocation},
    resource::domain::{Resource, ResourceId, StorageKey},
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
        StorageKey::from_resource_path(self.directory.path(), self.resource.name())
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

/// Rebuildable Resource query projection. It never owns aggregate writes.
#[async_trait::async_trait]
pub trait ResourceReadModel: Send + Sync {
    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<LocatedResource>, CoreError>;

    async fn find_by_directory_and_name(
        &self,
        directory_id: DirectoryId,
        name: &str,
    ) -> Result<Option<LocatedResource>, CoreError>;

    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError>;
}

/// Trusted full-catalog projection used only by storage maintenance. Normal business listing must
/// always be scoped to one Directory UUID through `ResourceReadModel::list`.
#[async_trait::async_trait]
pub trait ResourceMaintenanceReadModel: Send + Sync {
    async fn list_all(&self) -> Result<Vec<LocatedResource>, CoreError>;
}

/// Resource aggregate store. Every normal write has explicit insert or revision-CAS semantics.
#[async_trait::async_trait]
pub trait ResourceStore: Send + Sync {
    async fn health_check(&self) -> Result<(), CoreError>;

    async fn load(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError>;

    async fn insert(&self, resource: &Resource) -> Result<(), CoreError>;

    async fn update_if_revision(
        &self,
        resource: &Resource,
        expected_revision: u64,
    ) -> Result<bool, CoreError>;

    async fn delete_if_revision(
        &self,
        id: &ResourceId,
        expected_revision: u64,
    ) -> Result<bool, CoreError>;
}

/// Durable intent for a Resource rename/move whose physical and aggregate writes cannot share a
/// transaction. `desired` contains the post-relocation aggregate and therefore the next revision.
#[derive(Debug, Clone)]
pub struct ResourceRelocation {
    desired: Resource,
    expected_revision: u64,
    source_key: StorageKey,
    destination_key: StorageKey,
}

impl ResourceRelocation {
    pub fn new(
        desired: Resource,
        expected_revision: u64,
        source_key: StorageKey,
        destination_key: StorageKey,
    ) -> Result<Self, CoreError> {
        if source_key == destination_key {
            return Err(CoreError::invariant(
                "resource relocation source and destination must differ",
            ));
        }
        if desired.revision() <= expected_revision {
            return Err(CoreError::invariant(
                "resource relocation must advance the aggregate revision",
            ));
        }
        Ok(Self {
            desired,
            expected_revision,
            source_key,
            destination_key,
        })
    }

    pub fn resource_id(&self) -> ResourceId {
        self.desired.id()
    }

    pub fn desired(&self) -> &Resource {
        &self.desired
    }

    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub fn source_key(&self) -> &StorageKey {
        &self.source_key
    }

    pub fn destination_key(&self) -> &StorageKey {
        &self.destination_key
    }
}

#[async_trait::async_trait]
pub trait ResourceRelocationStore: Send + Sync {
    async fn save(&self, relocation: &ResourceRelocation) -> Result<(), CoreError>;
    async fn load_all(&self) -> Result<Vec<ResourceRelocation>, CoreError>;
    async fn complete(&self, id: &ResourceId) -> Result<(), CoreError>;
}
