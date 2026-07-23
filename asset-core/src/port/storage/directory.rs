//! 用户可见目录的存储侧端口。

use crate::CoreError;
use crate::domain::DirectoryPath;

/// 用户可见目录在对象存储中的持久化端口。
///
/// 文件系统实现创建真实目录；没有原生目录概念的对象存储实现应创建目录标记。
/// `.asset-hub` 属于内部 Blob 命名空间，不通过本端口管理。
#[async_trait::async_trait]
pub trait DirectoryStorage: Send + Sync {
    /// 幂等确保目录及其全部祖先在存储端存在。
    async fn ensure_directory(&self, directory: &DirectoryPath) -> Result<(), CoreError>;

    /// 原子移动或重命名一个完整目录子树；目标路径必须不存在。
    async fn move_directory(
        &self,
        _from: &DirectoryPath,
        _to: &DirectoryPath,
    ) -> Result<(), CoreError> {
        Err(CoreError::configuration(
            "the configured directory storage does not support directory moves",
        ))
    }
}
