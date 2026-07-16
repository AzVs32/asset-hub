//! 资源只读查询端口。
//!
//! 该端口承接分页、检索和目录浏览，不参与资源聚合的保存事务。

use crate::CoreError;
use crate::domain::{Resource, ResourceDirectory, ResourceKind, StorageKey};

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
    /// 按内容存储键查找资源，用于导入和扫描任务的幂等去重。
    async fn find_by_content_key(&self, key: &StorageKey) -> Result<Option<Resource>, CoreError>;

    /// 按条件分页列出资源。
    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError>;

    /// 列出指定父目录下的直接子目录。
    async fn list_directories(
        &self,
        parent: &ResourceDirectory,
    ) -> Result<Vec<ResourceDirectory>, CoreError>;
}
