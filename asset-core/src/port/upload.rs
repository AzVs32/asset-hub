use crate::CoreError;
use crate::domain::{Checksum, UploadId, UploadSession};

/// 上传会话持久化端口。
#[async_trait::async_trait]
pub trait UploadSessionRepository: Send + Sync {
    async fn save(&self, session: &UploadSession) -> Result<(), CoreError>;
    async fn find_by_id(&self, id: &UploadId) -> Result<Option<UploadSession>, CoreError>;
    async fn update_offset(
        &self,
        id: &UploadId,
        expected_offset: u64,
        offset: u64,
    ) -> Result<bool, CoreError>;
    async fn mark_finalizing(&self, id: &UploadId) -> Result<bool, CoreError>;
    async fn save_checksum(&self, id: &UploadId, checksum: &Checksum) -> Result<(), CoreError>;
    async fn mark_completed(&self, id: &UploadId) -> Result<(), CoreError>;
    async fn mark_failed(&self, id: &UploadId, failure: &str) -> Result<(), CoreError>;
    async fn list_finalizing(&self) -> Result<Vec<UploadId>, CoreError>;
    async fn remove(&self, id: &UploadId) -> Result<(), CoreError>;
}
