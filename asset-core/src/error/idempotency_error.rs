use thiserror::Error;

/// Validation errors for durable idempotency values.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdempotencyError {
    /// The client-provided key does not satisfy the persisted-key contract.
    #[error("{field} has invalid format: {reason}")]
    InvalidFormat {
        field: &'static str,
        reason: &'static str,
    },
}
