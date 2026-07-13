use crate::CoreError;
use crate::domain::{ResourceDirectory, StorageKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedBlob {
    pub key: StorageKey,
    pub size: u64,
    pub mime_type: Option<String>,
    pub sha256: Option<String>,
}

#[async_trait::async_trait]
pub trait StorageScanner: Send + Sync {
    async fn scan(
        &self,
        directory: &ResourceDirectory,
        include_sha256: bool,
        max_entries: usize,
    ) -> Result<Vec<ScannedBlob>, CoreError>;
}
