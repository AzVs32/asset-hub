use crate::CoreError;

#[async_trait::async_trait]
pub trait BlobStorage: Send + Sync {
    async fn put(&self, key: &str, data: bytes::Bytes) -> Result<(), CoreError>;

    async fn get(&self, key: &str) -> Result<bytes::Bytes, CoreError>;

    async fn delete(&self, key: &str) -> Result<(), CoreError>;

    async fn exists(&self, key: &str) -> Result<bool, CoreError>;
}
