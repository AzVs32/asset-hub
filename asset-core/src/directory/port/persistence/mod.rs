//! 目录聚合持久化、移动恢复、查询投影及可重建索引端口。

use crate::{
    CoreError,
    directory::{
        domain::{Directory, DirectoryId, DirectoryPath},
        query::LocatedDirectory,
    },
};

/// 原子目录更新批次中的一次乐观并发写入。
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryRevisionUpdate {
    directory: Directory,
    expected_revision: u64,
}

impl DirectoryRevisionUpdate {
    pub fn new(directory: Directory, expected_revision: u64) -> Result<Self, CoreError> {
        if directory.revision() <= expected_revision {
            return Err(CoreError::invariant(
                "a directory revision update must advance the revision",
            ));
        }
        Ok(Self {
            directory,
            expected_revision,
        })
    }

    pub fn directory(&self) -> &Directory {
        &self.directory
    }

    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }
}

/// 物理目录移动的持久化前向恢复意图。
///
/// 第一项更新始终是被移动的目录本身。
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryRelocation {
    directory_id: DirectoryId,
    source: DirectoryPath,
    destination: DirectoryPath,
    updates: Vec<DirectoryRevisionUpdate>,
}

impl DirectoryRelocation {
    pub fn new(
        directory_id: DirectoryId,
        source: DirectoryPath,
        destination: DirectoryPath,
        updates: Vec<DirectoryRevisionUpdate>,
    ) -> Result<Self, CoreError> {
        if source == destination {
            return Err(CoreError::invariant(
                "a directory relocation must change the physical path",
            ));
        }
        if updates.first().map(|update| update.directory().id()) != Some(directory_id) {
            return Err(CoreError::invariant(
                "the first relocation update must be the relocated directory",
            ));
        }
        if updates[0].directory().is_root() {
            return Err(CoreError::invariant("root directory cannot be relocated"));
        }
        Ok(Self {
            directory_id,
            source,
            destination,
            updates,
        })
    }

    pub fn directory_id(&self) -> DirectoryId {
        self.directory_id
    }

    pub fn source(&self) -> &DirectoryPath {
        &self.source
    }

    pub fn destination(&self) -> &DirectoryPath {
        &self.destination
    }

    pub fn updates(&self) -> &[DirectoryRevisionUpdate] {
        &self.updates
    }
}

/// 目录聚合的持久化端口。
///
/// 适配器只持久化聚合本身；完整路径和目录树属于可重建查询投影，不在此保存。
#[async_trait::async_trait]
pub trait DirectoryStore: Send + Sync {
    /// 加载全部目录聚合，用于启动时重建查询索引。
    async fn load_all(&self) -> Result<Vec<Directory>, CoreError>;

    /// 加载一个权威聚合，用于恢复和一致性检查。
    async fn load(&self, id: &DirectoryId) -> Result<Option<Directory>, CoreError>;

    /// 插入一个新目录聚合；ID 或同级名称冲突应返回 `CoreError::Conflict`。
    async fn insert(&self, directory: &Directory) -> Result<(), CoreError>;

    /// 仅当持久化版本仍等于 `expected_revision` 时原子保存聚合。
    ///
    /// 保存成功返回 `true`；记录不存在或版本已变化返回 `false`。
    /// 原子应用全部版本更新，或全部不应用。
    ///
    /// 空批次视为成功；聚合不存在或版本已过期时返回 `false`。
    async fn update_batch_if_unchanged(
        &self,
        updates: &[DirectoryRevisionUpdate],
    ) -> Result<bool, CoreError>;

    /// 返回目录当前是否既没有子目录，也没有资源。
    async fn is_empty(&self, id: &DirectoryId) -> Result<bool, CoreError>;

    /// 仅当目录不存在子目录和资源时原子删除；实际删除返回 `true`。
    async fn delete_if_empty(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
    ) -> Result<bool, CoreError>;
}

/// 在文件系统重命名开始前持久化的待处理物理目录移动。
#[async_trait::async_trait]
pub trait DirectoryRelocationStore: Send + Sync {
    /// 持久化新的目录移动及其全部原子数据库更新。
    async fn begin(&self, relocation: &DirectoryRelocation) -> Result<(), CoreError>;

    /// 加载所有未完成的目录移动，用于启动恢复。
    async fn load_pending(&self) -> Result<Vec<DirectoryRelocation>, CoreError>;

    /// 移除已经完成或可安全放弃的目录移动。
    async fn complete(&self, directory_id: &DirectoryId) -> Result<(), CoreError>;
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

/// 可从 `DirectoryStore` 完整重建的目录查询索引端口。
///
/// 应用服务只在持久化写入成功后更新该索引，因此实现不应把它视为权威数据源。
#[async_trait::async_trait]
pub trait DirectoryIndex: Send + Sync {
    /// 使用完整聚合集合重建并替换当前索引。
    async fn replace_all(&self, directories: Vec<Directory>) -> Result<(), CoreError>;

    /// 插入或替换单个目录投影，并刷新受影响的路径关系。
    async fn upsert(&self, directory: Directory) -> Result<(), CoreError>;

    /// 从索引中移除空目录；不得遗留引用该节点的子目录。
    async fn remove(&self, id: &DirectoryId) -> Result<(), CoreError>;
}

/// 同一投影对象分别实现读写端口时使用的适配器组合约定。
pub trait DirectoryProjection: DirectoryQuery + DirectoryIndex {}

impl<T> DirectoryProjection for T where T: DirectoryQuery + DirectoryIndex + ?Sized {}
