//! 资源命令服务。
//!
//! 本模块承接资源聚合本身的生命周期用例：更新、软删除和物理移除。
//! 它不直接处理预览渲染或插件动作，只在需要硬删除时协调对象内容清理。

use super::{ResourceService, UpdateResource};
use crate::CoreError;
use crate::domain::{DirectoryId, Resource, ResourceId, ResourceKind, StorageKey};
use crate::port::{
    DirectoryLocation, ListResources, LocatedResource, RESERVED_BLOB_STORAGE_PREFIX, ResourcePage,
};

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

        if let Some(tags) = command.tags {
            resource.replace_tags(tags)?;
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

    /// 软删除资源。
    ///
    /// 软删除会将对象内容移动到 Asset Hub 内部回收站，再更新资源聚合状态。原逻辑路径
    /// 会被释放，因此可以创建同目录、同名的新资源；恢复时内容会从回收站移回逻辑路径。
    ///
    /// 找不到资源时返回 `Ok(None)`；找到资源时返回保存后的资源状态。重复软删除同一资源是
    /// 幂等的，领域模型不会反复刷新删除时间。
    #[cfg(test)]
    pub(crate) async fn soft_delete_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(resource) = self.service.query.find_located_by_id(id).await? else {
            return Ok(None);
        };

        self.soft_delete_resource_snapshot(resource).await.map(Some)
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

    /// 物理移除资源及其对象内容。
    ///
    /// 该 usecase 用于维护任务或明确需要硬删除的场景，不是默认业务删除入口。
    ///
    /// 执行顺序是先按版本原子移除资源记录，再删除对象内容。这样并发更新不会导致新版本
    /// 引用的对象被误删；对象删除失败时，后续协调任务可发现并清理孤立对象。
    ///
    /// 返回值表示是否找到并尝试移除了资源：资源不存在时返回 `Ok(false)`，找到并完成移除时
    /// 返回 `Ok(true)`。
    #[cfg(test)]
    pub(crate) async fn remove_resource(&self, id: &ResourceId) -> Result<bool, CoreError> {
        let Some(resource) = self.service.query.find_located_by_id(id).await? else {
            return Ok(false);
        };

        self.remove_resource_snapshot(resource).await?;
        Ok(true)
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
    tags: Vec<String>,
) -> crate::domain::ResourceBuilder {
    let mut builder = Resource::builder(name)
        .with_directory_id(directory_id)
        .with_tags(tags);
    if let Some(kind) = kind {
        builder = builder.with_kind(kind);
    }
    builder
}
