//! 目录聚合应用服务。

use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryPath},
    port::{DirectoryLocation, DirectoryRepository, DirectoryStorage},
};
use std::sync::Arc;

/// 负责目录聚合、树关系与物理目录之间的用例编排。
#[derive(Clone)]
pub struct DirectoryService {
    repository: Arc<dyn DirectoryRepository>,
    storage: Arc<dyn DirectoryStorage>,
}

impl DirectoryService {
    pub fn new(
        repository: Arc<dyn DirectoryRepository>,
        storage: Arc<dyn DirectoryStorage>,
    ) -> Self {
        Self {
            repository,
            storage,
        }
    }

    pub async fn root(&self) -> Result<DirectoryLocation, CoreError> {
        self.repository
            .locate_by_id(&DirectoryId::root())
            .await?
            .ok_or_else(|| CoreError::configuration("root directory is missing"))
    }

    pub async fn locate_by_id(&self, id: &DirectoryId) -> Result<DirectoryLocation, CoreError> {
        self.repository
            .locate_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))
    }

    pub async fn resolve_path(&self, path: &DirectoryPath) -> Result<DirectoryLocation, CoreError> {
        self.repository
            .locate_by_path(path)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", path.path()))
    }

    /// 确保路径对应的每个目录聚合和物理目录都存在。
    pub async fn ensure_path(&self, path: &DirectoryPath) -> Result<DirectoryLocation, CoreError> {
        self.storage.ensure_directory(path).await?;
        self.repository.ensure_path(path).await
    }

    pub async fn list_children(
        &self,
        parent: &DirectoryLocation,
    ) -> Result<Vec<DirectoryLocation>, CoreError> {
        self.repository.list_children(&parent.id()).await
    }

    pub async fn create(
        &self,
        parent: &DirectoryLocation,
        name: impl Into<String>,
    ) -> Result<DirectoryLocation, CoreError> {
        let directory = Directory::new(parent.id(), name)?;
        let path = parent.path().child(directory.name())?;
        self.storage.ensure_directory(&path).await?;
        self.repository.save_directory(&directory).await?;
        Ok(DirectoryLocation::new(directory.id(), path))
    }

    pub async fn rename(
        &self,
        id: &DirectoryId,
        name: impl Into<String>,
    ) -> Result<DirectoryLocation, CoreError> {
        let directory = self
            .repository
            .find_directory(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))?;
        let parent_id = directory
            .parent_id()
            .ok_or_else(|| CoreError::conflict("root directory cannot be renamed"))?;
        self.relocate(directory, parent_id, Some(name.into())).await
    }

    pub async fn move_to(
        &self,
        id: &DirectoryId,
        parent_id: &DirectoryId,
    ) -> Result<DirectoryLocation, CoreError> {
        let directory = self
            .repository
            .find_directory(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))?;
        self.relocate(directory, *parent_id, None).await
    }

    pub async fn remove_if_empty(&self, directory: &DirectoryLocation) -> Result<bool, CoreError> {
        if directory.id().is_root() {
            return Ok(false);
        }
        self.repository.remove_if_empty(&directory.id()).await
    }

    pub async fn contains(
        &self,
        ancestor: &DirectoryId,
        candidate: &DirectoryId,
    ) -> Result<bool, CoreError> {
        self.repository
            .is_descendant_or_self(ancestor, candidate)
            .await
    }

    async fn relocate(
        &self,
        mut directory: Directory,
        parent_id: DirectoryId,
        name: Option<String>,
    ) -> Result<DirectoryLocation, CoreError> {
        let from = self
            .repository
            .locate_by_id(&directory.id())
            .await?
            .ok_or_else(|| CoreError::not_found("directory", directory.id().to_string()))?;
        let parent = self
            .repository
            .locate_by_id(&parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        if self
            .repository
            .is_descendant_or_self(&directory.id(), &parent_id)
            .await?
        {
            return Err(CoreError::conflict(
                "moving the directory would create a cycle",
            ));
        }
        if let Some(name) = name {
            directory.rename(name)?;
        }
        directory.move_to(parent_id)?;
        let destination = parent.path().child(directory.name())?;
        if destination == *from.path() {
            return Ok(from);
        }
        self.storage
            .move_directory(from.path(), &destination)
            .await?;
        if let Err(error) = self.repository.save_directory(&directory).await {
            if let Err(rollback) = self.storage.move_directory(&destination, from.path()).await {
                return Err(CoreError::storage("directory.relocate.rollback", rollback));
            }
            return Err(error);
        }
        Ok(DirectoryLocation::new(directory.id(), destination))
    }
}
