//! 目录聚合持久化和树查询端口。

use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryPath, DirectoryRef},
};

/// 目录聚合仓储。
///
/// 目录以 `id + parent_id` 保存；路径由实现根据祖先链派生。`ensure_path` 是面向文件
/// 系统导入和用户工作区创建的幂等边界，不会让路径成为目录身份。
#[async_trait::async_trait]
pub trait DirectoryRepository: Send + Sync {
    async fn save_directory(&self, directory: &Directory) -> Result<(), CoreError>;
    async fn find_directory(&self, id: &DirectoryId) -> Result<Option<Directory>, CoreError>;
    async fn locate_by_id(&self, id: &DirectoryId) -> Result<Option<DirectoryRef>, CoreError>;
    async fn locate_by_path(&self, path: &DirectoryPath)
    -> Result<Option<DirectoryRef>, CoreError>;
    async fn list_children(&self, parent_id: &DirectoryId) -> Result<Vec<DirectoryRef>, CoreError>;
    async fn ensure_path(&self, path: &DirectoryPath) -> Result<DirectoryRef, CoreError>;
    async fn remove_if_empty(&self, id: &DirectoryId) -> Result<bool, CoreError>;
    async fn is_descendant_or_self(
        &self,
        ancestor_id: &DirectoryId,
        candidate_id: &DirectoryId,
    ) -> Result<bool, CoreError>;
}
