//! 资源聚合仓储端口。
//!
//! 该端口描述核心层对“资源聚合持久化”的最小依赖，不绑定具体数据库实现。
//! sqlx 的 SQLite、Postgres 等实现应适配该 trait，而不是让应用层直接依赖数据库 API。

use crate::CoreError;
use crate::domain::{Resource, ResourceDirectory, ResourceId};
use chrono::{DateTime, Utc};

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

    /// Replace an existing aggregate only when it still has the expected version timestamp.
    /// Database implementations must perform the comparison and update atomically.
    async fn save_if_unchanged(
        &self,
        resource: &Resource,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError>;

    /// Remove an existing aggregate only when it still has the expected version timestamp.
    /// Database implementations must perform the comparison and delete atomically.
    async fn remove_if_unchanged(
        &self,
        id: &ResourceId,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<bool, CoreError>;

    /// 按资源 ID 查找资源聚合。
    ///
    /// 找不到记录时返回 `Ok(None)`。该方法不主动过滤软删除资源；调用方可通过
    /// `Resource::is_deleted()` 判断资源是否处于软删除状态。
    ///
    /// 该方法面向聚合还原，不承担复杂检索、分页或条件查询职责；只读查询统一通过
    /// `ResourceQuery` 完成。
    async fn find_by_id(&self, id: &ResourceId) -> Result<Option<Resource>, CoreError>;

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
