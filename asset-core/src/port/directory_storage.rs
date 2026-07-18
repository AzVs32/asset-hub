use crate::CoreError;
use crate::domain::ResourceDirectory;

/// 用户可见目录在对象存储中的持久化端口。
///
/// 文件系统实现创建真实目录；没有原生目录概念的对象存储实现应创建目录标记。
/// `.asset-hub` 属于内部 Blob 命名空间，不通过本端口管理。
#[async_trait::async_trait]
pub trait DirectoryStorage: Send + Sync {
    /// 幂等确保目录及其全部祖先在存储端存在。
    async fn ensure_directory(&self, directory: &ResourceDirectory) -> Result<(), CoreError>;
}
