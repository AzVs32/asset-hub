//! 资源聚合仓储端口。
//!
//! 该端口描述核心层对“资源聚合持久化”的最小依赖，不绑定具体数据库实现。
//! sqlx 的 SQLite、Postgres 等实现应适配该 trait，而不是让应用层直接依赖数据库 API。

use crate::CoreError;
use crate::domain::{Resource, ResourceDirectory, ResourceId, ResourceKind, StorageKey};

/// 资源列表查询条件。
#[derive(Debug, Clone)]
pub struct ListResources {
    /// 返回数量上限。
    limit: u32,
    /// 跳过的记录数量。
    offset: u64,
    /// 可选资源类型过滤。
    kinds: Vec<ResourceKind>,
    include_descendants: bool,
    /// 可选标签过滤。
    tag: Option<String>,
    /// 可选名称模糊搜索关键字。
    q: Option<String>,
    /// 可选逻辑目录过滤。
    directory: Option<ResourceDirectory>,
    /// 是否包含软删除资源。
    include_deleted: bool,
}

impl ListResources {
    /// 创建列表查询。
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

    /// 设置资源类型过滤。
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

    /// 设置标签过滤。
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// 设置名称模糊搜索。
    pub fn with_q(mut self, q: impl Into<String>) -> Self {
        self.q = Some(q.into());
        self
    }

    /// 设置逻辑目录过滤。
    pub fn with_directory(mut self, directory: ResourceDirectory) -> Self {
        self.directory = Some(directory);
        self
    }

    /// 设置是否包含软删除资源。
    pub fn with_include_deleted(mut self, include_deleted: bool) -> Self {
        self.include_deleted = include_deleted;
        self
    }

    /// 返回数量上限。
    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// 返回跳过记录数。
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// 返回资源类型过滤。
    pub fn kind(&self) -> Option<&ResourceKind> {
        self.kinds.first()
    }

    pub fn kinds(&self) -> &[ResourceKind] {
        &self.kinds
    }

    pub fn include_descendants(&self) -> bool {
        self.include_descendants
    }

    /// 返回标签过滤。
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// 返回名称搜索关键字。
    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    /// 返回逻辑目录过滤。
    pub fn directory(&self) -> Option<&ResourceDirectory> {
        self.directory.as_ref()
    }

    /// 返回是否包含软删除资源。
    pub fn include_deleted(&self) -> bool {
        self.include_deleted
    }
}

/// 资源分页查询结果。
#[derive(Debug, Clone)]
pub struct ResourcePage {
    /// 当前页资源。
    pub items: Vec<Resource>,
    /// 符合条件的总记录数。
    pub total: u64,
    /// 返回数量上限。
    pub limit: u32,
    /// 跳过的记录数量。
    pub offset: u64,
}

/// 资源聚合仓储端口。
///
/// `ResourceRepository` 负责保存和还原完整的 `Resource` 聚合，包括基础属性、元数据、
/// 内容引用和生命周期字段。它不负责对象内容本体的读写；对象内容应通过 `BlobStorage`
/// 处理。
///
/// 实现方从数据库读取记录后，应通过 `Resource::rehydrate` 还原聚合，确保历史数据仍然
/// 经过领域模型校验。底层数据库错误应转换为 `CoreError::Repository`。
#[async_trait::async_trait]
pub trait ResourceRepository: Send + Sync {
    /// 保存资源聚合的当前状态。
    ///
    /// 实现方应按 `ResourceId` 做 upsert：记录不存在时插入，已存在时更新。
    /// 该方法保存的是调用方传入聚合的完整当前状态，包括软删除时间和内容引用。
    ///
    /// 成功返回 `Ok(())`。唯一约束冲突、连接失败、SQL 执行失败等数据库层问题应返回
    /// `CoreError::Repository` 或更具体的 `CoreError::Conflict`。
    async fn save(&self, resource: &Resource) -> Result<(), CoreError>;

    /// 按资源 ID 查找资源聚合。
    ///
    /// 找不到记录时返回 `Ok(None)`。该方法不主动过滤软删除资源；调用方可通过
    /// `Resource::is_deleted()` 判断资源是否处于软删除状态。
    ///
    /// 该方法面向聚合还原，不承担复杂检索、分页或条件查询职责。后续若需要列表查询，
    /// 应单独增加查询端口，避免把聚合仓储扩成通用查询服务。
    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError>;

    /// 按内容存储键查找资源聚合。
    ///
    /// 该方法用于维护导入和扫描任务做幂等去重。找不到记录时返回 `Ok(None)`。
    async fn find_by_content_key(&self, key: &StorageKey) -> Result<Option<Resource>, CoreError>;

    /// 按条件分页列出资源。
    async fn list(&self, query: &ListResources) -> Result<ResourcePage, CoreError>;

    /// 列出指定父目录下的直接子目录。
    async fn list_directories(
        &self,
        parent_path: &str,
    ) -> Result<Vec<ResourceDirectory>, CoreError>;

    /// 保存一个可独立存在的逻辑目录。
    async fn save_directory(&self, _directory: &ResourceDirectory) -> Result<(), CoreError> {
        Err(CoreError::configuration(
            "directory persistence is not supported by this repository",
        ))
    }

    /// 从持久化存储中物理移除资源记录。
    ///
    /// 删除操作应保持幂等：记录不存在时也应视为删除成功。
    /// 业务软删除应通过 `Resource::soft_delete()` 修改聚合后再调用 `save()`。
    /// 该方法主要用于测试、维护任务或明确需要物理清理的场景，不应作为默认业务删除入口。
    async fn remove(&self, id: &ResourceId) -> Result<(), CoreError>;
}
