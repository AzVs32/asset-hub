//! Persistence port for durable business idempotency records.

use crate::CoreError;
use crate::domain::{IdempotencyKey, IdempotencyRecord};

#[async_trait::async_trait]
pub trait IdempotencyRepository: Send + Sync {
    /// Load the record for one key, if it exists.
    async fn find(&self, key: &IdempotencyKey) -> Result<Option<IdempotencyRecord>, CoreError>;

    /// Insert a new in-progress record. A duplicate key must return `CoreError::Conflict`.
    async fn insert(&self, record: &IdempotencyRecord) -> Result<(), CoreError>;

    /// Mark the record completed and persist its result.
    async fn complete(
        &self,
        key: &IdempotencyKey,
        result: serde_json::Value,
    ) -> Result<(), CoreError>;

    /// Remove a record for a failed attempt so the key can be retried.
    async fn remove(&self, key: &IdempotencyKey) -> Result<(), CoreError>;
}
