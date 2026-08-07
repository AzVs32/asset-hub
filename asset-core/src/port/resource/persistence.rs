//! 资源聚合持久化与只读查询端口。
//!
//! 查询端口承接资源分页与检索，不负责目录树查询，也不参与资源聚合的保存事务。

use crate::CoreError;
use crate::domain::{DirectoryId, DirectoryPath, Resource, ResourceId, ResourceKind, StorageKey};
use crate::port::DirectoryLocation;

/// 资源列表查询条件。
#[derive(Debug, Clone)]
pub struct ListResources {
    limit: u32,
    offset: u64,
    kinds: Vec<ResourceKind>,
    q: Option<String>,
    directory: Option<DirectoryPath>,
    directory_id: Option<DirectoryId>,
    include_deleted: bool,
}

impl ListResources {
    pub fn new(limit: u32, offset: u64) -> Self {
        Self {
            limit,
            offset,
            kinds: Vec::new(),
            q: None,
            directory: None,
            directory_id: None,
            include_deleted: false,
        }
    }

    pub fn with_kind(mut self, kind: ResourceKind) -> Self {
        self.kinds = vec![kind];
        self
    }

    pub fn with_kinds(mut self, kinds: Vec<ResourceKind>) -> Self {
        self.kinds = kinds;
        self
    }

    pub fn with_q(mut self, q: impl Into<String>) -> Self {
        self.q = Some(q.into());
        self
    }

    pub fn with_directory(mut self, directory: DirectoryPath) -> Self {
        self.directory = Some(directory);
        self.directory_id = None;
        self
    }

    pub fn with_directory_id(mut self, directory_id: DirectoryId) -> Self {
        self.directory_id = Some(directory_id);
        self.directory = None;
        self
    }

    pub fn with_include_deleted(mut self, include_deleted: bool) -> Self {
        self.include_deleted = include_deleted;
        self
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn kind(&self) -> Option<&ResourceKind> {
        self.kinds.first()
    }

    pub fn kinds(&self) -> &[ResourceKind] {
        &self.kinds
    }

    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    pub fn directory_id(&self) -> Option<&DirectoryId> {
        self.directory_id.as_ref()
    }

    pub fn directory(&self) -> Option<&DirectoryPath> {
        self.directory.as_ref()
    }

    pub fn include_deleted(&self) -> bool {
        self.include_deleted
    }
}

/// 带当前目录位置的资源读取投影。
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

/// 资源分页查询结果。
#[derive(Debug, Clone)]
pub struct ResourcePage {
    pub items: Vec<LocatedResource>,
    pub total: u64,
    pub limit: u32,
    pub offset: u64,
}

/// 资源读取投影端口。
///
/// 查询适配器负责组合资源聚合与当前目录位置，不承担聚合写入职责。
#[async_trait::async_trait]
pub trait ResourceQuery: Send + Sync {
    /// 按 ID 返回聚合及其当前目录位置，不过滤软删除状态。
    async fn find_located_by_id(
        &self,
        id: &ResourceId,
    ) -> Result<Option<LocatedResource>, CoreError>;

    /// 按逻辑目录和名称查找未软删除资源，用于导入和自动协调的幂等去重。
    ///
    /// 软删除资源的 Blob 已移入内部回收站，不再占用原逻辑路径，因此不应参与查找。
    async fn find_by_path(
        &self,
        directory: &DirectoryPath,
        name: &str,
    ) -> Result<Option<LocatedResource>, CoreError>;

    /// 按条件分页列出资源。
    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError>;
}

/// 资源聚合写仓储。
///
/// 只负责保存和还原完整 `Resource` 聚合；目录聚合与 Blob 内容分别由各自端口管理。
#[async_trait::async_trait]
pub trait ResourceRepository: Send + Sync {
    /// 检查资源持久化后端是否可访问；正常可用时返回 `Ok(())`。
    async fn health_check(&self) -> Result<(), CoreError>;

    /// 按 Resource ID 保存完整聚合状态。
    async fn save(&self, resource: &Resource) -> Result<(), CoreError>;

    /// 仅在聚合版本仍匹配时原子替换聚合。
    async fn save_if_unchanged(
        &self,
        resource: &Resource,
        expected_revision: u64,
    ) -> Result<bool, CoreError>;

    /// 仅在聚合版本仍匹配时原子删除聚合。
    async fn remove_if_unchanged(
        &self,
        id: &ResourceId,
        expected_revision: u64,
    ) -> Result<bool, CoreError>;

    /// 按 ID 还原聚合，不过滤软删除状态。
    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError>;

    /// 幂等物理移除资源记录。
    async fn remove(&self, id: &ResourceId) -> Result<(), CoreError>;
}
