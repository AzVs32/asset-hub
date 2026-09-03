//! Authorization-bound Resource metadata and lifecycle use cases.

use super::{ResourceService, UpdateResource};
use crate::CoreError;
use crate::domain::{AccessContext, DirectoryOperation, DirectoryPath, Resource, ResourceId};
use crate::port::{DirectoryLocation, ListResources, LocatedResource, ResourcePage};
use crate::service::AuthorizationService;

pub struct SecuredResourceService<'a> {
    service: &'a ResourceService,
    authorization: &'a AuthorizationService,
    context: &'a AccessContext,
}

impl<'a> SecuredResourceService<'a> {
    pub(super) fn new(
        service: &'a ResourceService,
        authorization: &'a AuthorizationService,
        context: &'a AccessContext,
    ) -> Self {
        Self {
            service,
            authorization,
            context,
        }
    }

    async fn require(
        &self,
        directory: &DirectoryLocation,
        operation: DirectoryOperation,
    ) -> Result<(), CoreError> {
        self.authorization
            .require(self.context, directory, operation)
            .await
    }

    async fn resource_for(
        &self,
        id: &ResourceId,
        operation: DirectoryOperation,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let resource = self.service.get(id).await?;
        if let Some(resource) = &resource {
            self.require(resource.directory(), operation).await?;
        }
        Ok(resource)
    }

    pub async fn get(&self, id: &ResourceId) -> Result<Option<LocatedResource>, CoreError> {
        self.resource_for(id, DirectoryOperation::ReadResource)
            .await
    }

    /// Resolve the caller-relative path at the authorization boundary, then query only by UUID.
    pub async fn list(
        &self,
        requested_directory: &DirectoryPath,
        query: ListResources,
    ) -> Result<ResourcePage, CoreError> {
        let absolute = self
            .authorization
            .workspace_scope(self.context)
            .await?
            .resolve(requested_directory)?;
        let directory = self.service.directories.resolve_path(&absolute).await?;
        self.require(&directory, DirectoryOperation::ReadResource)
            .await?;
        let query = ListResources::new(query.limit(), query.offset(), directory.id())
            .with_kinds(query.kinds().to_vec());
        let query = query
            .q()
            .map(str::to_string)
            .map(|q| query.clone().with_q(q))
            .unwrap_or(query);
        self.service.list(query).await
    }

    pub async fn update(
        &self,
        id: &ResourceId,
        command: UpdateResource,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(resource) = self
            .resource_for(id, DirectoryOperation::UpdateResource)
            .await?
        else {
            return Ok(None);
        };
        if let Some(target_id) = command.directory_id() {
            let target = self.service.directories.locate_by_id(&target_id).await?;
            self.require(&target, DirectoryOperation::UpdateResource)
                .await?;
        }
        self.service.update(resource, command).await.map(Some)
    }

    pub async fn delete(&self, id: &ResourceId, expected_revision: u64) -> Result<bool, CoreError> {
        let Some(resource) = self
            .resource_for(id, DirectoryOperation::DeleteResource)
            .await?
        else {
            return Ok(false);
        };
        self.service.delete(resource, expected_revision).await?;
        Ok(true)
    }
}
