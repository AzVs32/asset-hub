//! Persistence port for crash-recoverable permanent Resource deletions.

use crate::CoreError;
use crate::resource::domain::{ResourceDeletion, ResourceId};

#[async_trait::async_trait]
pub trait ResourceDeletionRepository: Send + Sync {
    /// Persist an intent before moving a visible Blob into the internal deletion namespace.
    /// Repeating the exact same intent must succeed so a caller can retry after a database or
    /// cleanup failure; a different pending intent for the same Resource must conflict.
    async fn save(&self, deletion: &ResourceDeletion) -> Result<(), CoreError>;

    /// Return every pending deletion in stable Resource-ID order.
    async fn list_pending(&self) -> Result<Vec<ResourceDeletion>, CoreError>;

    /// Idempotently clear an intent only after the deletion committed or a revision conflict was
    /// safely resolved without overwriting the newer Resource state.
    async fn remove(&self, resource_id: &ResourceId) -> Result<(), CoreError>;
}
