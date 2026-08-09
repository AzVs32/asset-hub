//! Directory aggregate persistence, query projections, and rebuildable index ports.

use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryPath},
};

/// A directory's stable identity and current path projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryLocation {
    id: DirectoryId,
    path: DirectoryPath,
}

impl DirectoryLocation {
    pub fn new(id: DirectoryId, path: DirectoryPath) -> Self {
        Self { id, path }
    }

    pub fn root() -> Self {
        Self::new(DirectoryId::root(), DirectoryPath::root())
    }

    pub fn id(&self) -> DirectoryId {
        self.id
    }

    pub fn path(&self) -> &DirectoryPath {
        &self.path
    }
}

/// A complete directory aggregate paired with its current path projection.
#[derive(Debug, Clone, PartialEq)]
pub struct LocatedDirectory {
    directory: Directory,
    location: DirectoryLocation,
}

impl LocatedDirectory {
    pub fn new(directory: Directory, location: DirectoryLocation) -> Result<Self, CoreError> {
        if directory.id() != location.id() {
            return Err(CoreError::invariant(
                "directory aggregate does not match its location projection",
            ));
        }
        Ok(Self {
            directory,
            location,
        })
    }

    pub fn directory(&self) -> &Directory {
        &self.directory
    }

    pub fn location(&self) -> &DirectoryLocation {
        &self.location
    }

    pub fn id(&self) -> DirectoryId {
        self.directory.id()
    }

    pub fn path(&self) -> &DirectoryPath {
        self.location.path()
    }

    pub fn into_directory(self) -> Directory {
        self.directory
    }

    pub fn into_parts(self) -> (Directory, DirectoryLocation) {
        (self.directory, self.location)
    }
}

/// 目录聚合的持久化端口。
///
/// 适配器只持久化聚合本身；完整路径和目录树属于可重建查询投影，不在此保存。
#[async_trait::async_trait]
pub trait DirectoryRepository: Send + Sync {
    /// 加载全部目录聚合，用于启动时重建查询索引。
    async fn load_all(&self) -> Result<Vec<Directory>, CoreError>;

    /// 插入一个新目录聚合；ID 或同级名称冲突应返回 `CoreError::Conflict`。
    async fn insert(&self, directory: &Directory) -> Result<(), CoreError>;

    /// 仅当持久化版本仍等于 `expected_revision` 时原子保存聚合。
    ///
    /// 保存成功返回 `true`；记录不存在或版本已变化返回 `false`。
    async fn save_if_unchanged(
        &self,
        directory: &Directory,
        expected_revision: u64,
    ) -> Result<bool, CoreError>;

    /// 仅当目录不存在子目录和资源时原子删除；实际删除返回 `true`。
    async fn remove_if_empty(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
    ) -> Result<bool, CoreError>;
}

/// 目录树的只读查询投影端口。
///
/// 查询适配器负责根据目录聚合构造稳定 ID 与当前完整路径一致的 `LocatedDirectory`。
#[async_trait::async_trait]
pub trait DirectoryQuery: Send + Sync {
    /// 按稳定目录 ID 查询聚合及当前位置；不存在时返回 `None`。
    async fn find_by_id(&self, id: &DirectoryId) -> Result<Option<LocatedDirectory>, CoreError>;

    /// 按当前规范路径查询目录；不存在时返回 `None`。
    async fn find_by_path(
        &self,
        path: &DirectoryPath,
    ) -> Result<Option<LocatedDirectory>, CoreError>;

    /// 返回指定目录的直接子目录；父目录不存在或没有子目录时返回空集合。
    async fn list_children(
        &self,
        parent_id: &DirectoryId,
    ) -> Result<Vec<LocatedDirectory>, CoreError>;

    /// 判断 `candidate_id` 是否为 `ancestor_id` 本身或其任意层级后代。
    async fn is_descendant_or_self(
        &self,
        ancestor_id: &DirectoryId,
        candidate_id: &DirectoryId,
    ) -> Result<bool, CoreError>;
}

/// 可从 `DirectoryRepository` 完整重建的目录查询索引端口。
///
/// Service 只在持久化写入成功后更新该索引，因此实现不应把它视为权威数据源。
#[async_trait::async_trait]
pub trait DirectoryIndex: DirectoryQuery {
    /// 使用完整聚合集合重建并替换当前索引。
    async fn replace_all(&self, directories: Vec<Directory>) -> Result<(), CoreError>;

    /// 插入或替换单个目录投影，并刷新受影响的路径关系。
    async fn upsert(&self, directory: Directory) -> Result<(), CoreError>;

    /// 从索引中移除空目录；不得遗留引用该节点的子目录。
    async fn remove(&self, id: &DirectoryId) -> Result<(), CoreError>;
}
