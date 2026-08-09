//! Authorization-bound Directory use cases for untrusted application surfaces.

use super::{DirectoryService, ExecuteDirectoryAction, UpdateDirectory};
use crate::{
    CoreError,
    domain::{AccessContext, DirectoryId, DirectoryKind, DirectoryOperation, DirectoryPath},
    port::{DirectoryActionOutput, DirectoryLocation, LocatedDirectory},
    service::AuthorizationService,
};

pub struct SecuredDirectoryService<'a> {
    service: &'a DirectoryService,
    authorization: &'a AuthorizationService,
    context: &'a AccessContext,
}

impl<'a> SecuredDirectoryService<'a> {
    pub(super) fn new(
        service: &'a DirectoryService,
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

    async fn resolve(&self, path: &DirectoryPath) -> Result<DirectoryPath, CoreError> {
        self.authorization
            .workspace_scope(self.context)
            .await?
            .resolve(path)
    }

    pub async fn find_by_id(&self, id: &DirectoryId) -> Result<LocatedDirectory, CoreError> {
        let directory = self.service.find_by_id(id).await?;
        self.require(directory.location(), DirectoryOperation::ViewDirectory)
            .await?;
        Ok(directory)
    }

    pub async fn find_by_path(&self, path: &DirectoryPath) -> Result<LocatedDirectory, CoreError> {
        let path = self.resolve(path).await?;
        let directory = self.service.find_by_path(&path).await?;
        self.require(directory.location(), DirectoryOperation::ViewDirectory)
            .await?;
        Ok(directory)
    }

    pub async fn list_children(
        &self,
        path: &DirectoryPath,
    ) -> Result<Vec<LocatedDirectory>, CoreError> {
        let directory = self.find_by_path(path).await?;
        self.service.list_located_children(&directory.id()).await
    }

    pub async fn create(
        &self,
        parent_id: &DirectoryId,
        name: impl Into<String>,
        kind: DirectoryKind,
    ) -> Result<LocatedDirectory, CoreError> {
        let parent = self.service.find_by_id(parent_id).await?;
        self.require(parent.location(), DirectoryOperation::CreateDirectory)
            .await?;
        let scope_root = self
            .authorization
            .workspace_scope(self.context)
            .await?
            .root()
            .id();
        self.service
            .create_with_kind_in_scope(parent.location(), name, kind, scope_root)
            .await
    }

    pub async fn update(
        &self,
        id: &DirectoryId,
        command: UpdateDirectory,
    ) -> Result<LocatedDirectory, CoreError> {
        let directory = self.service.find_by_id(id).await?;
        self.require(directory.location(), DirectoryOperation::UpdateDirectory)
            .await?;
        let scope_root = self
            .authorization
            .workspace_scope(self.context)
            .await?
            .root()
            .id();
        self.service
            .update_expected(id, command, Some(scope_root))
            .await
    }

    pub async fn remove_if_empty(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
    ) -> Result<bool, CoreError> {
        let directory = self.service.find_by_id(id).await?;
        self.require(directory.location(), DirectoryOperation::DeleteDirectory)
            .await?;
        self.service
            .remove_if_empty(directory.location(), Some(expected_revision))
            .await
    }

    pub async fn execute_action(
        &self,
        id: &DirectoryId,
        command: ExecuteDirectoryAction,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let directory = self.service.find_by_id(id).await?;
        self.service
            .resolve_action(directory.directory(), &command.action)?;
        self.require(
            directory.location(),
            DirectoryOperation::ExecuteDirectoryAction,
        )
        .await?;
        let scope_root = self
            .authorization
            .workspace_scope(self.context)
            .await?
            .root()
            .id();
        let executed = self.service.invoke_action(id, command).await?;
        self.service
            .apply_executed_action(&executed, Some(scope_root))
            .await?;
        Ok(executed.into_output())
    }
}
