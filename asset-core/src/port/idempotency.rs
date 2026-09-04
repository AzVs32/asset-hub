//! Persistence port for durable business idempotency records.

use crate::CoreError;
use crate::domain::{IdempotencyExecutionId, IdempotencyKey, IdempotencyRecord};

/// Atomic result of attempting to obtain a persisted execution lease.
pub enum IdempotencyAcquire {
    /// This caller created a record or conditionally took over an expired lease.
    Acquired,
    /// A non-acquirable record remains persisted and should determine the caller's response.
    Existing(IdempotencyRecord),
    /// The record disappeared after a failed acquire (for example, its owner abandoned it).
    /// Callers may safely retry acquisition.
    Vacant,
}

#[async_trait::async_trait]
pub trait IdempotencyRepository: Send + Sync {
    /// Atomically create a record or take over the same-request record only after its lease has
    /// expired. Implementations must not perform an unguarded select-then-update takeover.
    async fn acquire(&self, record: &IdempotencyRecord) -> Result<IdempotencyAcquire, CoreError>;

    /// Mark the record completed and persist its result.
    async fn complete(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
        result: serde_json::Value,
    ) -> Result<bool, CoreError>;

    /// Remove a record for a failed attempt so the key can be retried.
    async fn remove(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
    ) -> Result<bool, CoreError>;

    /// Extend the lease held by one execution. Returns false if that execution no longer owns it.
    async fn renew(
        &self,
        key: &IdempotencyKey,
        execution_id: IdempotencyExecutionId,
        lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CoreError>;
}
