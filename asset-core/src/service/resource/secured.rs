//! 绑定授权上下文的资源服务入口。
//!
//! 非可信调用方先在这里完成目录权限校验，再进入内部 command/content/action
//! 编排，避免每个 transport 重复实现授权规则。

use super::{
    CreateUpload, DirectoryArchiveManifest, DirectoryArchiveResource, ExecuteResourceAction,
    ReplaceResourceContent, ResourceContentStream, ResourceService, UpdateResource,
};
use crate::CoreError;
use crate::domain::{
    AccessContext, Checksum, DirectoryId, DirectoryOperation, DirectoryPath, Resource, ResourceId,
    UploadId, UploadSession,
};
use crate::port::{
    BlobByteStream, DirectoryLocation, ListResources, LocatedResource, ResourceActionOutput,
    ResourcePage,
};
use crate::service::AuthorizationService;
use bytes::Bytes;
use std::collections::VecDeque;

const DIRECTORY_ARCHIVE_PAGE_SIZE: u32 = 100;

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
        directory: &DirectoryLocation,
        operation: DirectoryOperation,
    ) -> Result<(), CoreError> {
        self.authorization
            .require(self.context, directory, operation)
            .await
    }
    async fn require_resource(
        &self,
        resource: &LocatedResource,
        operation: DirectoryOperation,
    ) -> Result<(), CoreError> {
        self.require(resource.directory(), operation).await
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
        operation: DirectoryOperation,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let resource = self.service.commands().find_resource(id).await?;
        if let Some(resource) = &resource {
            self.require_resource(resource, operation).await?;
        }
        Ok(resource)
    }
    async fn stored_resource_for(
        &self,
        id: &ResourceId,
        operation: DirectoryOperation,
    ) -> Result<Option<LocatedResource>, CoreError> {
        let resource = self.service.query.find_located_by_id(id).await?;
        if let Some(resource) = &resource {
            self.require_resource(resource, operation).await?;
        }
        Ok(resource)
    }
    pub async fn create_upload(
        &self,
        mut command: CreateUpload,
    ) -> Result<UploadSession, CoreError> {
        let directory = self.resolve(command.directory()).await?;
        command = command.with_directory(directory);
        self.service
            .uploads()
            .create(self.context.user_id(), command)
            .await
    }
    pub async fn upload_status(&self, id: &UploadId) -> Result<UploadSession, CoreError> {
        self.service
            .uploads()
            .status(self.context.user_id(), id)
            .await
    }
    pub async fn append_upload(
        &self,
        id: &UploadId,
        offset: u64,
        expected_chunk_checksum: Checksum,
        data: BlobByteStream,
    ) -> Result<UploadSession, CoreError> {
        self.service
            .uploads()
            .append(
                self.context.user_id(),
                id,
                offset,
                expected_chunk_checksum,
                data,
            )
            .await
    }
    pub async fn complete_upload(&self, id: &UploadId) -> Result<UploadSession, CoreError> {
        let (session, _) = self
            .service
            .uploads()
            .request_finalization(self.context.user_id(), id)
            .await?;
        Ok(session)
    }
    pub async fn abort_upload(&self, id: &UploadId) -> Result<(), CoreError> {
        self.service
            .uploads()
            .abort(self.context.user_id(), id)
            .await
    }
    pub async fn find_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<LocatedResource>, CoreError> {
        self.resource_for(id, DirectoryOperation::ReadResource)
            .await
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
    pub async fn directory_archive_manifest(
        &self,
        id: &DirectoryId,
    ) -> Result<DirectoryArchiveManifest, CoreError> {
        let root = self.service.directories.find_by_id(id).await?;
        self.require(root.location(), DirectoryOperation::DownloadDirectory)
            .await?;
        let archive_root = if root.id().is_root() {
            "asset-hub".to_string()
        } else {
            root.directory().name().to_string()
        };
        let filename = format!("{archive_root}.zip");
        let root_path = root.path().path().to_string();
        let mut pending = VecDeque::from([root]);
        let mut directories = Vec::new();
        let mut resources = Vec::new();

        while let Some(directory) = pending.pop_front() {
            if !self
                .service
                .directories
                .contains(id, &directory.id())
                .await?
            {
                continue;
            }
            let archive_path =
                directory_archive_path(&archive_root, &root_path, directory.path().path())?;
            directories.push(format!("{archive_path}/"));

            let mut offset = 0;
            loop {
                let page = self
                    .service
                    .commands()
                    .list_resources(
                        ListResources::new(DIRECTORY_ARCHIVE_PAGE_SIZE, offset)
                            .with_directory_id(directory.id()),
                    )
                    .await?;
                let item_count = page.items.len() as u64;
                resources.extend(page.items.into_iter().filter_map(|located| {
                    let resource = located.resource();
                    resource.content().map(|_| {
                        DirectoryArchiveResource::new(
                            resource.id(),
                            format!("{archive_path}/{}", resource.name()),
                        )
                    })
                }));
                offset += item_count;
                if offset >= page.total || item_count == 0 {
                    break;
                }
            }

            pending.extend(
                self.service
                    .directories
                    .list_located_children(&directory.id())
                    .await?,
            );
        }

        directories.sort();
        resources.sort_by(|left, right| left.path().cmp(right.path()));
        Ok(DirectoryArchiveManifest::new(
            filename,
            directories,
            resources,
        ))
    }
    pub async fn update_resource(
        &self,
        id: &ResourceId,
        mut command: UpdateResource,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(resource) = self
            .stored_resource_for(id, DirectoryOperation::UpdateResource)
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
        let Some(resource) = self
            .resource_for(id, DirectoryOperation::ReadResource)
            .await?
        else {
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
        let Some(resource) = self
            .resource_for(id, DirectoryOperation::ReadResource)
            .await?
        else {
            return Ok(None);
        };
        self.service
            .content()
            .get_resource_content_stream_snapshot(&resource, range)
            .await
    }
    pub async fn replace_resource_content(
        &self,
        id: &ResourceId,
        command: ReplaceResourceContent,
        data: BlobByteStream,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(resource) = self
            .stored_resource_for(id, DirectoryOperation::ReplaceResourceContent)
            .await?
        else {
            return Ok(None);
        };
        self.service
            .content()
            .replace_text_content_snapshot(resource, command, data)
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
        let definition = self
            .service
            .actions()
            .resolve_declared_resource_action(resource.resource(), &command.action)?;
        let operation = if definition
            .output()
            .effects
            .iter()
            .any(|effect| effect == "delete")
        {
            DirectoryOperation::DeleteResource
        } else {
            DirectoryOperation::ExecuteResourceAction
        };
        self.require_resource(&resource, operation).await?;
        self.service
            .actions()
            .execute_resource_action_snapshot(resource, command)
            .await
            .map(Some)
    }
    pub async fn soft_delete_resource(
        &self,
        id: &ResourceId,
        expected_revision: u64,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(resource) = self
            .stored_resource_for(id, DirectoryOperation::DeleteResource)
            .await?
        else {
            return Ok(None);
        };
        if resource.resource().revision() != expected_revision {
            return Err(CoreError::revision_conflict("resource", id.to_string()));
        }
        self.service
            .commands()
            .soft_delete_resource_snapshot(resource)
            .await
            .map(Some)
    }
    pub async fn remove_resource(&self, id: &ResourceId) -> Result<bool, CoreError> {
        let Some(resource) = self
            .stored_resource_for(id, DirectoryOperation::PurgeResource)
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

fn directory_archive_path(
    archive_root: &str,
    root_path: &str,
    directory_path: &str,
) -> Result<String, CoreError> {
    if directory_path == root_path {
        return Ok(archive_root.to_string());
    }
    let relative = if root_path.is_empty() {
        directory_path
    } else {
        directory_path
            .strip_prefix(root_path)
            .and_then(|suffix| suffix.strip_prefix('/'))
            .ok_or_else(|| CoreError::invariant("directory left the archive subtree"))?
    };
    Ok(format!("{archive_root}/{relative}"))
}
