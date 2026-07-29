//! 对象存储端口。
//!
//! 该端口描述核心层对“对象内容存取”的最小依赖，不绑定具体存储实现。
//! OpenDAL 的 Fs、S3 等能力应通过该 trait 适配进来，应用层只依赖这里定义的语义。

use crate::CoreError;
use crate::domain::StorageKey;
use bytes::Bytes;
use futures_core::Stream;
use std::pin::Pin;

/// 对象内容字节流。
///
/// 该类型用于大文件上传场景。每个 chunk 都是已经从调用入口读取到的一段二进制内容；
/// stream 中的错误会中止写入，并由具体存储适配器负责清理未完成写入。
pub type BlobByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, CoreError>> + Send + 'static>>;

/// Blob storage namespace reserved for Asset Hub internals.
///
/// User-managed resources must not use this prefix. Infrastructure adapters and scanners use the
/// same value to keep internal action scratch objects out of user-visible imports.
pub const RESERVED_BLOB_STORAGE_PREFIX: &str = crate::domain::INTERNAL_STORAGE_DIRECTORY_NAME;

/// 已完整写入内部暂存区、尚未发布到用户可见路径的 Blob。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedBlob {
    key: StorageKey,
    bytes_written: u64,
}

impl StagedBlob {
    /// 由存储适配器创建暂存句柄。
    pub fn new(key: StorageKey, bytes_written: u64) -> Self {
        Self { key, bytes_written }
    }

    /// 返回内部暂存对象键。
    pub fn key(&self) -> &StorageKey {
        &self.key
    }

    /// 返回暂存对象的实际字节数。
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

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
    /// 检查配置的对象存储命名空间是否可访问；正常可用时返回 `Ok(())`。
    async fn health_check(&self) -> Result<(), CoreError>;

    /// 写入或覆盖指定存储键对应的对象内容。
    ///
    /// 该方法的语义是 upsert：对象不存在时创建，已存在时覆盖。是否允许覆盖由上层
    /// usecase 决定；端口实现只需按存储系统能力完成写入。
    ///
    /// 成功返回 `Ok(())`。存储系统不可用、权限不足、网络失败、磁盘写入失败等情况
    /// 应返回 `CoreError::Storage`。
    async fn put(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError>;

    /// 创建一个空的内部暂存对象。
    async fn create_staged(&self, key: &StorageKey) -> Result<StagedBlob, CoreError>;

    /// 在实际长度与 `expected_offset` 一致时追加内容。
    ///
    /// 返回成功前必须完成 flush、持久化同步并关闭写入句柄。
    async fn append_staged(
        &self,
        key: &StorageKey,
        expected_offset: u64,
        data: BlobByteStream,
    ) -> Result<StagedBlob, CoreError>;

    /// 检查暂存对象的实际长度。
    async fn inspect_staged(&self, key: &StorageKey) -> Result<Option<StagedBlob>, CoreError>;

    /// 将完整暂存对象原子发布到目标键，且不得覆盖已有目标。
    ///
    /// 成功发布后暂存对象仍然存在，由调用方在 Resource 保存完成后显式清理。
    async fn publish_staged_if_absent(
        &self,
        staged: &StagedBlob,
        target: &StorageKey,
    ) -> Result<(), CoreError>;

    /// 幂等清理内部暂存对象。
    async fn discard_staged(&self, staged: &StagedBlob) -> Result<(), CoreError>;

    /// 读取指定存储键对应的对象内容。
    ///
    /// 当对象不存在时返回 `Ok(None)`，这表示“正常查无结果”。只有存储系统自身故障
    /// 才返回 `Err`，例如连接失败、权限不足或读取过程中发生 I/O 错误。
    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, CoreError>;

    /// 流式读取指定存储键对应的对象内容。
    ///
    /// 用于预览、下载等大对象读取场景，避免把完整对象一次性加载到内存中。
    async fn get_stream(&self, key: &StorageKey) -> Result<Option<BlobByteStream>, CoreError>;

    /// 流式读取指定存储键的一段字节范围，范围是闭区间 `[start, end]`。
    ///
    /// 调用方负责保证 `start <= end` 且范围不超过对象大小。实现只负责从底层存储
    /// 请求对应范围并以流返回，避免为了 HTTP Range 响应加载完整对象。
    async fn get_range_stream(
        &self,
        key: &StorageKey,
        start: u64,
        end: u64,
    ) -> Result<Option<BlobByteStream>, CoreError>;

    /// 仅当目标键不存在时移动对象。
    ///
    /// 成功后源键不再存在；目标键已经存在时返回 `CoreError::Conflict`，不得覆盖。
    async fn move_if_absent(&self, from: &StorageKey, to: &StorageKey) -> Result<(), CoreError>;

    /// 删除指定存储键对应的对象。
    ///
    /// 删除操作必须保持幂等：对象不存在时也应返回 `Ok(())`。这能让上层 usecase
    /// 在补偿删除、重复清理或任务重试时不需要额外区分对象是否已经被移除。
    async fn delete(&self, key: &StorageKey) -> Result<(), CoreError>;
}
