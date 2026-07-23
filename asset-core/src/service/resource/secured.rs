//! 绑定授权上下文的资源服务入口。
//!
//! 非可信调用方先在这里完成目录权限校验，再进入内部 command/content/action/preview
//! 编排，避免每个 transport 重复实现授权规则。

use super::{
    CreateResource, ExecuteResourceAction, ReadableResource, ResourceContentStream,
    ResourcePreviewStream, ResourceService, ResourceThumbnail, UpdateResource,
    UploadResourceContentStream,
};
use crate::CoreError;
use crate::domain::{
    AccessContext, DirectoryPath, DirectoryPermission, DirectoryRef, Resource, ResourceId,
};
use crate::port::{ListResources, ResourceActionOutput, ResourcePage};
use crate::service::AuthorizationService;
use asset_plugin_api::ResourceActionAccess;
use bytes::Bytes;

/// 绑定访问主体后的资源用例门面。外部用户入口应使用本门面，可信维护任务可继续使用原服务。
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
        directory: &DirectoryRef,
        permission: DirectoryPermission,
    ) -> Result<(), CoreError> {
        self.authorization
            .require(self.context, directory, permission)
            .await
    }
    async fn resolve(&self, directory: &DirectoryPath) -> Result<DirectoryPath, CoreError> {
        self.authorization
            .workspace_scope(self.context)
            .await?
            .resolve(directory)
    }
    async fn resource_for(
        &self,
        id: &ResourceId,
        permission: DirectoryPermission,
    ) -> Result<Option<Resource>, CoreError> {
        let resource = self.service.commands().find_resource(id).await?;
        if let Some(resource) = &resource {
            self.require(resource.directory(), permission).await?;
        }
        Ok(resource)
    }
    async fn stored_resource_for(
        &self,
        id: &ResourceId,
        permission: DirectoryPermission,
    ) -> Result<Option<Resource>, CoreError> {
        let resource = self.service.repository.find_by_id(id).await?;
        if let Some(resource) = &resource {
            self.require(resource.directory(), permission).await?;
        }
        Ok(resource)
    }
    pub async fn create_resource(
        &self,
        mut command: CreateResource,
    ) -> Result<Resource, CoreError> {
        let directory = self.resolve(command.directory()).await?;
        command = command.with_directory(directory);
        self.service.commands().create_resource(command).await
    }
    pub async fn upload_resource_content_stream(
        &self,
        mut command: UploadResourceContentStream,
    ) -> Result<Resource, CoreError> {
        let directory = self.resolve(command.directory()).await?;
        command = command.with_directory(directory);
        self.service
            .content()
            .upload_resource_content_stream(command)
            .await
    }
    pub async fn find_resource(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        self.resource_for(id, DirectoryPermission::Read).await
    }
    pub async fn list_resources(
        &self,
        mut query: ListResources,
    ) -> Result<ResourcePage, CoreError> {
        let requested_directory = query.directory().cloned().unwrap_or_default();
        let directory = self.resolve(&requested_directory).await?;
        let directory = self.service.directories.resolve_path(&directory).await?;
        query = query.with_directory_id(directory.id());
        self.service.commands().list_resources(query).await
    }
    pub async fn list_directories(
        &self,
        directory: &DirectoryPath,
    ) -> Result<Vec<DirectoryRef>, CoreError> {
        let directory = self.resolve(directory).await?;
        let directory = self.service.directories.resolve_path(&directory).await?;
        self.service.directories.list_children(&directory).await
    }
    pub async fn create_directory(
        &self,
        parent: &DirectoryPath,
        name: impl Into<String>,
    ) -> Result<DirectoryRef, CoreError> {
        let parent = self.resolve(parent).await?;
        let parent = self.service.directories.resolve_path(&parent).await?;
        self.service.directories.create(&parent, name).await
    }
    pub async fn update_resource(
        &self,
        id: &ResourceId,
        mut command: UpdateResource,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(resource) = self
            .stored_resource_for(id, DirectoryPermission::Write)
            .await?
        else {
            return Ok(None);
        };
        if let Some(directory) = command.directory().cloned() {
            command = command.with_directory(self.resolve(&directory).await?);
        }
        self.service
            .commands()
            .update_resource_snapshot(resource, command)
            .await
            .map(Some)
    }
    pub async fn get_resource_content(&self, id: &ResourceId) -> Result<Option<Bytes>, CoreError> {
        let Some(resource) = self.resource_for(id, DirectoryPermission::Read).await? else {
            return Ok(None);
        };
        self.service
            .content()
            .get_resource_content_snapshot(&resource)
            .await
    }

    pub async fn get_resource_content_stream(
        &self,
        id: &ResourceId,
        range: Option<(u64, u64)>,
    ) -> Result<Option<ResourceContentStream>, CoreError> {
        let Some(resource) = self.resource_for(id, DirectoryPermission::Read).await? else {
            return Ok(None);
        };
        self.service
            .content()
            .get_resource_content_stream_snapshot(&resource, range)
            .await
    }
    pub async fn read_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ReadableResource>, CoreError> {
        let Some(resource) = self.resource_for(id, DirectoryPermission::Read).await? else {
            return Ok(None);
        };
        self.service
            .previews()
            .read_resource_snapshot(resource)
            .await
            .map(Some)
    }
    pub async fn preview_resource_stream(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourcePreviewStream>, CoreError> {
        let Some(resource) = self.resource_for(id, DirectoryPermission::Read).await? else {
            return Ok(None);
        };
        self.service
            .previews()
            .preview_resource_stream_snapshot(&resource)
            .await
            .map(Some)
    }
    pub async fn thumbnail_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourceThumbnail>, CoreError> {
        let Some(resource) = self.resource_for(id, DirectoryPermission::Read).await? else {
            return Ok(None);
        };
        self.service
            .previews()
            .thumbnail_resource_snapshot(resource)
            .await
            .map(Some)
    }
    pub async fn execute_resource_action(
        &self,
        id: &ResourceId,
        command: ExecuteResourceAction,
    ) -> Result<Option<ResourceActionOutput>, CoreError> {
        let resource = self.service.commands().find_resource(id).await?;
        let Some(resource) = resource else {
            return Ok(None);
        };
        let access = self
            .service
            .actions()
            .resolve_declared_resource_action(&resource, &command.action)?
            .access();
        let permission = match access {
            ResourceActionAccess::ReadOnly => DirectoryPermission::Read,
            ResourceActionAccess::ReadWrite => DirectoryPermission::Write,
        };
        self.require(resource.directory(), permission).await?;
        self.service
            .actions()
            .execute_resource_action_snapshot(resource, command)
            .await
            .map(Some)
    }
    pub async fn soft_delete_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(resource) = self
            .stored_resource_for(id, DirectoryPermission::Write)
            .await?
        else {
            return Ok(None);
        };
        self.service
            .commands()
            .soft_delete_resource_snapshot(resource)
            .await
            .map(Some)
    }
    pub async fn remove_resource(&self, id: &ResourceId) -> Result<bool, CoreError> {
        let Some(resource) = self
            .stored_resource_for(id, DirectoryPermission::Full)
            .await?
        else {
            return Ok(false);
        };
        self.service
            .commands()
            .remove_resource_snapshot(resource)
            .await?;
        Ok(true)
    }
}
