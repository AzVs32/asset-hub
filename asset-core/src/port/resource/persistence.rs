//! 资源聚合持久化与只读查询端口。
//!
//! 该端口承接分页、检索和目录浏览，不参与资源聚合的保存事务。

use crate::CoreError;
use crate::domain::{Resource, ResourceDirectory, ResourceId, ResourceKind};
use chrono::{DateTime, Utc};

/// 资源列表查询条件。
#[derive(Debug, Clone)]
pub struct ListResources {
    limit: u32,
    offset: u64,
    kinds: Vec<ResourceKind>,
    include_descendants: bool,
    tag: Option<String>,
    q: Option<String>,
    directory: Option<ResourceDirectory>,
    include_deleted: bool,
}

impl ListResources {
    pub fn new(limit: u32, offset: u64) -> Self {
        Self {
            limit,
            offset,
            kinds: Vec::new(),
            include_descendants: false,
            tag: None,
            q: None,
            directory: None,
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

    pub fn with_include_descendants(mut self, include_descendants: bool) -> Self {
        self.include_descendants = include_descendants;
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    pub fn with_q(mut self, q: impl Into<String>) -> Self {
        self.q = Some(q.into());
        self
    }

    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = Some(directory);
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

    pub fn include_descendants(&self) -> bool {
        self.include_descendants
    }

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    pub fn directory(&self) -> Option<&ResourceDirectory> {
        self.directory.as_ref()
    }

    pub fn include_deleted(&self) -> bool {
        self.include_deleted
    }
}

/// 资源分页查询结果。
#[derive(Debug, Clone)]
pub struct ResourcePage {
    pub items: Vec<Resource>,
    pub total: u64,
    pub limit: u32,
    pub offset: u64,
}

#[async_trait::async_trait]
pub trait ResourceQuery: Send + Sync {
    /// 按逻辑目录和名称查找未软删除资源，用于导入和自动协调的幂等去重。
    ///
    /// 软删除资源的 Blob 已移入内部回收站，不再占用原逻辑路径，因此不应参与查找。
    async fn find_by_path(
        &self,
        directory: &ResourceDirectory,
        name: &str,
    ) -> Result<Option<Resource>, CoreError>;

    /// 按条件分页列出资源。
    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError>;

    /// 列出指定父目录下的直接子目录。
    async fn list_directories(
        &self,
        parent: &ResourceDirectory,
    ) -> Result<Vec<ResourceDirectory>, CoreError>;
}

/// 资源聚合写仓储。
///
/// 负责保存和还原完整 `Resource` 聚合以及用户可见目录记录；Blob 内容由存储端口管理。
#[async_trait::async_trait]
pub trait ResourceRepository: Send + Sync {
    async fn health_check(&self) -> Result<(), CoreError>;

    /// 按 Resource ID 保存完整聚合状态。
    async fn save(&self, resource: &Resource) -> Result<(), CoreError>;

    /// 仅在版本时间仍匹配时原子替换聚合。
    async fn save_if_unchanged(
        &self,
        resource: &Resource,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError>;

    /// 仅在版本时间仍匹配时原子删除聚合。
    async fn remove_if_unchanged(
        &self,
        id: &ResourceId,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError>;

    /// 按 ID 还原聚合，不过滤软删除状态。
    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError>;

    /// 保存已在存储侧创建的独立目录。
    async fn save_directory(&self, directory: &ResourceDirectory) -> Result<(), CoreError>;

    /// 幂等保存目录及其祖先链。
    async fn ensure_directory(&self, directory: &ResourceDirectory) -> Result<(), CoreError>;

    /// 删除不再存在于存储侧的空目录记录。
    async fn remove_directory(&self, directory: &ResourceDirectory) -> Result<(), CoreError>;

    /// 幂等物理移除资源记录。
    async fn remove(&self, id: &ResourceId) -> Result<(), CoreError>;
}
