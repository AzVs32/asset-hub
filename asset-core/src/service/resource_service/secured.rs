use super::*;
use crate::domain::{AccessContext, DirectoryPermission};
use crate::service::AuthorizationService;

/// 绑定访问主体后的资源用例门面。外部用户入口应使用本门面，可信维护任务可继续使用原服务。
pub struct SecuredResourceService<'a> {
    service: &'a ResourceService,
    authorization: &'a AuthorizationService,
    context: &'a AccessContext,
}

impl<'a> SecuredResourceService<'a> {
    pub fn new(
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
        directory: &str,
        permission: DirectoryPermission,
    ) -> Result<(), CoreError> {
        self.authorization
            .require(self.context, directory, permission)
            .await
    }
    async fn resource_for(
        &self,
        id: &ResourceId,
        permission: DirectoryPermission,
    ) -> Result<Option<Resource>, CoreError> {
        let resource = self.service.find_resource(id).await?;
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
    pub async fn create_resource(&self, command: CreateResource) -> Result<Resource, CoreError> {
        self.require(command.directory(), DirectoryPermission::Write)
            .await?;
        self.service.create_resource(command).await
    }
    pub async fn upload_resource_content(
        &self,
        command: UploadResourceContent,
    ) -> Result<Resource, CoreError> {
        self.require(command.directory(), DirectoryPermission::Write)
            .await?;
        self.service.upload_resource_content(command).await
    }
    pub async fn import_resource_content(
        &self,
        command: ImportResourceContent,
    ) -> Result<Option<Resource>, CoreError> {
        self.require(command.directory(), DirectoryPermission::Write)
            .await?;
        self.service.import_resource_content(command).await
    }
    pub async fn upload_resource_content_stream(
        &self,
        command: UploadResourceContentStream,
    ) -> Result<Resource, CoreError> {
        self.require(command.directory(), DirectoryPermission::Write)
            .await?;
        self.service.upload_resource_content_stream(command).await
    }
    pub async fn find_resource(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError> {
        self.resource_for(id, DirectoryPermission::Read).await
    }
    pub async fn list_resources(&self, query: ListResources) -> Result<ResourcePage, CoreError> {
        self.require(query.directory().unwrap_or(""), DirectoryPermission::Read)
            .await?;
        self.service.list_resources(query).await
    }
    pub async fn list_directories(
        &self,
        directory: &str,
    ) -> Result<Vec<ResourceDirectory>, CoreError> {
        self.require(directory, DirectoryPermission::Read).await?;
        self.service.list_directories(directory).await
    }
    pub async fn create_directory(
        &self,
        parent_path: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<ResourceDirectory, CoreError> {
        let parent_path = crate::domain::normalize_directory(parent_path.into())?;
        self.require(&parent_path, DirectoryPermission::Write)
            .await?;
        self.service.create_directory(parent_path, name).await
    }
    pub async fn update_resource(
        &self,
        id: &ResourceId,
        command: UpdateResource,
    ) -> Result<Option<Resource>, CoreError> {
        if self
            .stored_resource_for(id, DirectoryPermission::Write)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        if let Some(directory) = command.directory() {
            self.require(directory, DirectoryPermission::Write).await?;
        }
        self.service.update_resource(id, command).await
    }
    pub async fn get_resource_content(&self, id: &ResourceId) -> Result<Option<Bytes>, CoreError> {
        if self
            .resource_for(id, DirectoryPermission::Read)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.service.get_resource_content(id).await
    }
    pub async fn read_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ReadableResource>, CoreError> {
        if self
            .resource_for(id, DirectoryPermission::Read)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.service.read_resource(id).await
    }
    pub async fn preview_resource_stream(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourcePreviewStream>, CoreError> {
        if self
            .resource_for(id, DirectoryPermission::Read)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.service.preview_resource_stream(id).await
    }
    pub async fn thumbnail_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<ResourceThumbnail>, CoreError> {
        if self
            .resource_for(id, DirectoryPermission::Read)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.service.thumbnail_resource(id).await
    }
    pub async fn execute_resource_action(
        &self,
        id: &ResourceId,
        command: ExecuteResourceAction,
    ) -> Result<Option<ResourceActionOutput>, CoreError> {
        let resource = self.service.find_resource(id).await?;
        let Some(resource) = resource else {
            return Ok(None);
        };
        let actions = self.service.describe_resource_actions(&resource)?;
        let access = actions
            .available_actions()
            .iter()
            .find(|a| a.id().as_str() == command.action.as_str())
            .map(|a| a.access())
            .unwrap_or(crate::port::ResourceActionAccess::ReadOnly);
        let permission = match access {
            crate::port::ResourceActionAccess::ReadOnly => DirectoryPermission::Read,
            crate::port::ResourceActionAccess::ReadWrite => DirectoryPermission::Write,
        };
        self.require(resource.directory(), permission).await?;
        self.service.execute_resource_action(id, command).await
    }
    pub async fn soft_delete_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<Resource>, CoreError> {
        if self
            .stored_resource_for(id, DirectoryPermission::Write)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.service.soft_delete_resource(id).await
    }
    pub async fn remove_resource(&self, id: &ResourceId) -> Result<bool, CoreError> {
        if self
            .stored_resource_for(id, DirectoryPermission::Manage)
            .await?
            .is_none()
        {
            return Ok(false);
        }
        self.service.remove_resource(id).await
    }
}
