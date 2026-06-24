//! 对象存储端口。
//!
//! 该端口描述核心层对“对象内容存取”的最小依赖，不绑定具体存储实现。
//! OpenDAL 的 Fs、S3 等能力应通过该 trait 适配进来，应用层只依赖这里定义的语义。

use crate::CoreError;
use crate::domain::StorageKey;

/// 对象内容存储端口。
///
/// `BlobStorage` 只负责存取二进制内容本体，不负责维护 `Resource` 聚合、
/// 元数据、生命周期状态或数据库事务。调用方应使用已经通过领域校验的
/// `StorageKey`，避免对象键格式规则散落在具体存储适配器中。
///
/// 实现方应将 OpenDAL、文件系统、S3 等底层错误转换为 `CoreError::Storage`，
/// 不应把具体基础设施错误类型暴露到端口签名中。
#[async_trait::async_trait]
pub trait BlobStorage: Send + Sync {
    /// 写入或覆盖指定存储键对应的对象内容。
    ///
    /// 该方法的语义是 upsert：对象不存在时创建，已存在时覆盖。是否允许覆盖由上层
    /// usecase 决定；端口实现只需按存储系统能力完成写入。
    ///
    /// 成功返回 `Ok(())`。存储系统不可用、权限不足、网络失败、磁盘写入失败等情况
    /// 应返回 `CoreError::Storage`。
    async fn put(&self, key: &StorageKey, data: bytes::Bytes) -> Result<(), CoreError>;

    /// 读取指定存储键对应的对象内容。
    ///
    /// 当对象不存在时返回 `Ok(None)`，这表示“正常查无结果”。只有存储系统自身故障
    /// 才返回 `Err`，例如连接失败、权限不足或读取过程中发生 I/O 错误。
    async fn get(&self, key: &StorageKey) -> Result<Option<bytes::Bytes>, CoreError>;

    /// 删除指定存储键对应的对象。
    ///
    /// 删除操作必须保持幂等：对象不存在时也应返回 `Ok(())`。这能让上层 usecase
    /// 在补偿删除、重复清理或任务重试时不需要额外区分对象是否已经被移除。
    async fn delete(&self, key: &StorageKey) -> Result<(), CoreError>;

    /// 判断指定存储键对应的对象是否存在。
    ///
    /// 该方法只表达对象内容是否存在，不判断对应的 `Resource` 是否存在、是否软删除、
    /// 或是否仍然引用该对象。
    async fn exists(&self, key: &StorageKey) -> Result<bool, CoreError>;
}
