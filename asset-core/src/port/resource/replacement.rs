//! Persistence port for crash-recoverable Resource content replacement intents.

use crate::CoreError;
use crate::domain::{ResourceContentReplacement, ResourceContentReplacementId};

#[async_trait::async_trait]
pub trait ResourceContentReplacementRepository: Send + Sync {
    /// Insert one pending replacement. Implementations must allow at most one per Resource.
    async fn save(&self, replacement: &ResourceContentReplacement) -> Result<(), CoreError>;

    /// Return every pending replacement in stable creation order.
    async fn list_pending(&self) -> Result<Vec<ResourceContentReplacement>, CoreError>;

    /// Idempotently remove a completed or rolled-back replacement intent.
    async fn remove(&self, id: &ResourceContentReplacementId) -> Result<(), CoreError>;
}
