//! 资源命令服务。
//!
//! 本模块承接资源聚合本身的生命周期用例：创建、更新、软删除和物理移除。
//! 它不直接处理预览渲染或插件动作，只在需要硬删除时协调对象内容清理。

use super::*;

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

    /// 创建不包含对象内容的资源。
    ///
    /// 该 usecase 只保存资源聚合，不写入对象存储。成功时返回已经保存的 `Resource`，
    /// 其中包含新生成的 `ResourceId`、创建时间和更新时间。
    ///
    /// 可能返回的错误包括领域校验错误和仓储保存错误。
    pub(crate) async fn create_resource(
        &self,
        command: CreateResource,
    ) -> Result<Resource, CoreError> {
        let kind = self.service.validate_registered_kind(command.kind)?;
        let resource = build_resource(
            command.name,
            command.directory,
            Some(kind),
            command.status,
            command.description,
            command.tags,
        )
        .build()?;

        self.service.repository.save(&resource).await?;

        Ok(resource)
    }

    /// 按 ID 查找资源。
    ///
    /// 找不到资源或资源已经软删除时返回 `Ok(None)`。维护类操作需要读取软删除资源时，
    /// 应使用专门的恢复或物理删除用例。
    pub(crate) async fn find_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<Resource>, CoreError> {
        Ok(self
            .service
            .repository
            .find_by_id(id)
            .await?
            .filter(|resource| !resource.is_deleted()))
    }

    /// 分页列出资源。
    pub(crate) async fn list_resources(
        &self,
        mut query: ListResources,
    ) -> Result<ResourcePage, CoreError> {
        for kind in query.kinds() {
            self.service.ensure_kind_registered(kind)?;
        }
        if query.include_descendants()
            && let Some(kind) = query.kind().cloned()
        {
            query = query.with_kinds(self.service.kind_registry.descendants(&kind));
        }

        self.service.query.list(&query).await
    }

    /// 列出指定父目录下的直接子目录。
    pub(crate) async fn list_directories(
        &self,
        parent: &ResourceDirectory,
    ) -> Result<Vec<ResourceDirectory>, CoreError> {
        self.service.query.list_directories(parent).await
    }

    /// 在指定父目录下创建一个可独立存在的逻辑目录。
    pub(crate) async fn create_directory(
        &self,
        parent: &ResourceDirectory,
        name: impl Into<String>,
    ) -> Result<ResourceDirectory, CoreError> {
        let directory = parent.child(name)?;
        self.service.repository.save_directory(&directory).await?;
        Ok(directory)
    }

    /// 更新资源基础信息、元数据、状态，或恢复软删除资源。
    pub(crate) async fn update_resource_snapshot(
        &self,
        mut resource: Resource,
        command: UpdateResource,
    ) -> Result<Resource, CoreError> {
        let expected_updated_at = resource.updated_at();
        let old_storage_key = resource.content().map(|_| resource.storage_key());

        if command.restore {
            resource.restore();
        }

        if let Some(name) = command.name {
            resource.rename(name)?;
        }

        if let Some(directory) = command.directory {
            resource.move_to_directory(directory)?;
        }

        if let Some(kind) = command.kind {
            resource.change_kind(self.service.validate_registered_kind(Some(kind))?)?;
        }

        if let Some(status) = command.status {
            match status {
                ResourceStatus::Active => resource.activate()?,
                ResourceStatus::Archived => resource.archive()?,
            }
        }

        if let Some(description) = command.description {
            resource.set_description(description)?;
        }

        if let Some(tags) = command.tags {
            resource.replace_tags(tags)?;
        }

        let new_storage_key = resource.content().map(|_| resource.storage_key());
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
            .save_if_unchanged(&resource, expected_updated_at)
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
    /// 软删除只更新资源聚合状态并保存到仓储，不删除对象存储中的内容。这样可以保留恢复、
    /// 审计或异步清理的空间。
    ///
    /// 找不到资源时返回 `Ok(None)`；找到资源时返回保存后的资源状态。重复软删除同一资源是
    /// 幂等的，领域模型不会反复刷新删除时间。
    #[cfg(test)]
    pub(crate) async fn soft_delete_resource(
        &self,
        id: &ResourceId,
    ) -> Result<Option<Resource>, CoreError> {
        let Some(resource) = self.service.repository.find_by_id(id).await? else {
            return Ok(None);
        };

        self.soft_delete_resource_snapshot(resource).await.map(Some)
    }

    pub(crate) async fn soft_delete_resource_snapshot(
        &self,
        mut resource: Resource,
    ) -> Result<Resource, CoreError> {
        let expected_updated_at = resource.updated_at();

        resource.soft_delete();
        if !self
            .service
            .repository
            .save_if_unchanged(&resource, expected_updated_at)
            .await?
        {
            return Err(CoreError::conflict(format!(
                "resource `{}` changed while it was being deleted",
                resource.id()
            )));
        }

        Ok(resource)
    }

    /// 物理移除资源及其对象内容。
    ///
    /// 该 usecase 用于维护任务或明确需要硬删除的场景，不是默认业务删除入口。
    ///
    /// 执行顺序是先按版本原子移除资源记录，再删除对象内容。这样并发更新不会导致新版本
    /// 引用的对象被误删；对象删除失败时，审计任务可发现并清理孤立对象。
    ///
    /// 返回值表示是否找到并尝试移除了资源：资源不存在时返回 `Ok(false)`，找到并完成移除时
    /// 返回 `Ok(true)`。
    #[cfg(test)]
    pub(crate) async fn remove_resource(&self, id: &ResourceId) -> Result<bool, CoreError> {
        let Some(resource) = self.service.repository.find_by_id(id).await? else {
            return Ok(false);
        };

        self.remove_resource_snapshot(resource).await?;
        Ok(true)
    }

    pub(crate) async fn remove_resource_snapshot(
        &self,
        resource: Resource,
    ) -> Result<(), CoreError> {
        if !self
            .service
            .repository
            .remove_if_unchanged(&resource.id(), resource.updated_at())
            .await?
        {
            return Err(CoreError::conflict(format!(
                "resource `{}` changed while it was being removed",
                resource.id()
            )));
        }

        if resource.content().is_some() {
            self.service
                .blob_storage
                .delete(&resource.storage_key())
                .await?;
        }

        Ok(())
    }
}
