//! 资源命令服务。
//!
//! 本模块承接资源聚合本身的生命周期用例：更新、软删除和物理移除。
//! 它不直接处理预览渲染或插件动作，只在需要硬删除时协调对象内容清理。

use super::{ResourceService, UpdateResource};
use crate::CoreError;
use crate::domain::{Checksum, DirectoryId, Resource, ResourceId, ResourceKind, StorageKey};
use crate::port::{
    DirectoryLocation, ListResources, LocatedResource, RESERVED_BLOB_STORAGE_PREFIX, ResourcePage,
};
use bytes::Bytes;
use sha2::{Digest, Sha256};

/// 资源命令服务。
///
/// 该服务以 `ResourceService` 门面作为上下文，复用其中注入的仓储、对象存储和 kind registry。
pub(super) struct ResourceCommandService<'a> {
    service: &'a ResourceService,
}

impl<'a> ResourceCommandService<'a> {
    /// 创建资源命令服务。
    pub(super) fn new(service: &'a ResourceService) -> Self {
        Self { service }
    }

    /// 按 ID 查找资源。
    ///
    /// 找不到资源或资源已经软删除时返回 `Ok(None)`。维护类操作需要读取软删除资源时，
    /// 应使用专门的恢复或物理删除用例。
    pub(crate) async fn find_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<LocatedResource>, CoreError> {
        Ok(self
            .service
            .query
            .find_located_by_id(id)
            .await?
            .filter(|located| !located.resource().is_deleted()))
    }

    /// Creates one small Host-generated resource without exposing storage authority to a plugin.
    pub(crate) async fn create_generated_resource_snapshot(
        &self,
        directory: &DirectoryLocation,
        name: String,
        kind: Option<ResourceKind>,
        mime_type: Option<String>,
        data: Bytes,
    ) -> Result<LocatedResource, CoreError> {
        let storage_key = StorageKey::from_resource_path(directory.path(), &name)?;
        let kind = self.service.resolve_content_kind(
            kind,
            mime_type.as_deref(),
            Some(storage_key.as_str()),
        )?;
        let mut resource = build_resource(name.clone(), directory.id(), Some(kind)).build()?;
        let checksum = Checksum::sha256(super::content::hex_digest(&Sha256::digest(&data)))?;
        let content =
            super::content::build_verified_content(data.len() as u64, mime_type, checksum, None)?;
        resource.attach_content(content)?;

        let _storage_guard = self.service.storage_key_locks.lock(&storage_key).await;
        if self
            .service
            .query
            .find_by_path(directory.path(), &name)
            .await?
            .is_some()
        {
            return Err(CoreError::conflict(format!(
                "resource path `{storage_key}` already exists"
            )));
        }

        let staging_key = StorageKey::new(format!(
            "{RESERVED_BLOB_STORAGE_PREFIX}/uploads/generated-{}",
            uuid::Uuid::now_v7()
        ))?;
        let staging = self
            .service
            .blob_storage
            .create_staged(&staging_key)
            .await?;
        let expected_size = data.len() as u64;
        let staged = match self
            .service
            .blob_storage
            .append_staged(
                &staging_key,
                0,
                Box::pin(futures_util::stream::once(async move { Ok(data) })),
            )
            .await
        {
            Ok(staged) if staged.bytes_written() == expected_size => staged,
            Ok(staged) => {
                let _ = self.service.blob_storage.discard_staged(&staged).await;
                return Err(CoreError::conflict(format!(
                    "generated content size mismatch: expected {expected_size}, received {}",
                    staged.bytes_written()
                )));
            }
            Err(error) => {
                let _ = self.service.blob_storage.discard_staged(&staging).await;
                return Err(error);
            }
        };

        if let Err(error) = self
            .service
            .blob_storage
            .publish_staged_if_absent(&staged, &storage_key)
            .await
        {
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(error);
        }
        if let Err(error) = self.service.repository.save(&resource).await {
            let _ = self.service.blob_storage.delete(&storage_key).await;
            let _ = self.service.blob_storage.discard_staged(&staged).await;
            return Err(error);
        }
        let _ = self.service.blob_storage.discard_staged(&staged).await;
        LocatedResource::new(resource, directory.clone())
    }

    /// 分页列出资源。
    pub(crate) async fn list_resources(
        &self,
        mut query: ListResources,
    ) -> Result<ResourcePage, CoreError> {
        for kind in query.kinds() {
            self.service.ensure_kind_registered(kind)?;
        }
        if let Some(kind) = query.kind().cloned() {
            query = query.with_kinds(self.service.kind_registry.descendants(&kind));
        }

        self.service.query.list(&query).await
    }

    /// 更新资源基础信息或恢复软删除资源。
    pub(crate) async fn update_resource_snapshot(
        &self,
        located: LocatedResource,
        command: UpdateResource,
    ) -> Result<Resource, CoreError> {
        let (mut resource, mut directory) = located.into_parts();
        let expected_revision = resource.revision();
        if command.expected_revision != expected_revision {
            return Err(CoreError::revision_conflict(
                "resource",
                resource.id().to_string(),
            ));
        }
        let old_storage_key = persisted_content_key(&resource, &directory)?;
        let restoring = resource.is_deleted() && command.restore;

        if command.restore {
            resource.restore();
        }

        if let Some(name) = command.name {
            resource.rename(name)?;
        }

        if let Some(target_directory) = command.directory {
            directory = self
                .service
                .directories
                .ensure_path(&target_directory)
                .await?;
            resource.move_to_directory(directory.id())?;
        }

        if let Some(kind) = command.kind {
            resource.change_kind(self.service.validate_registered_kind(Some(kind))?)?;
        }

        let new_storage_key = persisted_content_key(&resource, &directory)?;
        let lock_keys = old_storage_key
            .iter()
            .chain(new_storage_key.iter())
            .cloned()
            .collect::<Vec<_>>();
        let _storage_guards = self.service.storage_key_locks.lock_many(&lock_keys).await;

        if restoring
            && self
                .service
                .query
                .find_by_path(directory.path(), resource.name())
                .await?
                .is_some()
        {
            return Err(CoreError::conflict(format!(
                "resource path `{}` is already occupied",
                StorageKey::from_resource_path(directory.path(), resource.name())?
            )));
        }

        let moved_content = match (&old_storage_key, &new_storage_key) {
            (Some(from), Some(to)) if from != to => {
                self.service.blob_storage.move_if_absent(from, to).await?;
                true
            }
            _ => false,
        };

        let saved = self
            .service
            .repository
            .save_if_unchanged(&resource, expected_revision)
            .await;

        let error = match saved {
            Ok(true) => return Ok(resource),
            Ok(false) => CoreError::conflict(format!(
                "resource `{}` changed while it was being updated",
                resource.id()
            )),
            Err(error) => error,
        };

        if moved_content
            && let (Some(from), Some(to)) = (&old_storage_key, &new_storage_key)
            && let Err(rollback_error) = self.service.blob_storage.move_if_absent(to, from).await
        {
            return Err(CoreError::storage(
                "resource_path_update.rollback",
                rollback_error,
            ));
        }

        Err(error)
    }

    pub(crate) async fn soft_delete_resource_snapshot(
        &self,
        located: LocatedResource,
    ) -> Result<Resource, CoreError> {
        let (mut resource, directory) = located.into_parts();
        let expected_revision = resource.revision();
        let old_storage_key = persisted_content_key(&resource, &directory)?;

        resource.soft_delete();
        let new_storage_key = persisted_content_key(&resource, &directory)?;
        let lock_keys = old_storage_key
            .iter()
            .chain(new_storage_key.iter())
            .cloned()
            .collect::<Vec<_>>();
        let _storage_guards = self.service.storage_key_locks.lock_many(&lock_keys).await;
        let moved_content = match (&old_storage_key, &new_storage_key) {
            (Some(from), Some(to)) if from != to => {
                self.service.blob_storage.move_if_absent(from, to).await?;
                true
            }
            _ => false,
        };

        let saved = self
            .service
            .repository
            .save_if_unchanged(&resource, expected_revision)
            .await;
        let error = match saved {
            Ok(true) => return Ok(resource),
            Ok(false) => CoreError::conflict(format!(
                "resource `{}` changed while it was being deleted",
                resource.id()
            )),
            Err(error) => error,
        };

        if moved_content
            && let (Some(from), Some(to)) = (&old_storage_key, &new_storage_key)
            && let Err(rollback_error) = self.service.blob_storage.move_if_absent(to, from).await
        {
            return Err(CoreError::storage(
                "resource_soft_delete.rollback",
                rollback_error,
            ));
        }

        Err(error)
    }

    pub(crate) async fn remove_resource_snapshot(
        &self,
        located: LocatedResource,
    ) -> Result<(), CoreError> {
        let (resource, directory) = located.into_parts();
        let storage_key = persisted_content_key(&resource, &directory)?;
        let _storage_guard = if let Some(storage_key) = &storage_key {
            Some(self.service.storage_key_locks.lock(storage_key).await)
        } else {
            None
        };
        if !self
            .service
            .repository
            .remove_if_unchanged(&resource.id(), resource.revision())
            .await?
        {
            return Err(CoreError::conflict(format!(
                "resource `{}` changed while it was being removed",
                resource.id()
            )));
        }

        if let Some(storage_key) = storage_key {
            self.service.blob_storage.delete(&storage_key).await?;
        }

        Ok(())
    }
}

/// 返回资源内容当前实际占用的对象键。
///
/// 活动资源使用其逻辑路径；软删除资源使用按资源 ID 隔离的内部回收站路径。
fn persisted_content_key(
    resource: &Resource,
    directory: &DirectoryLocation,
) -> Result<Option<StorageKey>, CoreError> {
    if resource.content().is_none() {
        return Ok(None);
    }

    if resource.is_deleted() {
        return StorageKey::new(format!(
            "{RESERVED_BLOB_STORAGE_PREFIX}/trash/{}",
            resource.id()
        ))
        .map(Some)
        .map_err(CoreError::from);
    }

    StorageKey::from_resource_path(directory.path(), resource.name())
        .map(Some)
        .map_err(Into::into)
}

pub(super) fn build_resource(
    name: String,
    directory_id: DirectoryId,
    kind: Option<ResourceKind>,
) -> crate::domain::ResourceBuilder {
    let mut builder = Resource::builder(name).with_directory_id(directory_id);
    if let Some(kind) = kind {
        builder = builder.with_kind(kind);
    }
    builder
}
