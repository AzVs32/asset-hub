use thiserror::Error;

/// Storage value validation errors independent of any business aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StorageError {
    /// A required storage value is empty or contains only whitespace.
    #[error("{field} cannot be blank")]
    Blank { field: &'static str },

    /// A storage value exceeds the supported character length.
    #[error("{field} must not exceed {max} characters")]
    TooLong { field: &'static str, max: usize },

    /// A storage value violates its canonical format.
    #[error("{field} has invalid format: {reason}")]
    InvalidFormat {
        field: &'static str,
        reason: &'static str,
    },
}
