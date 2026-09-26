use thiserror::Error;

/// Errors from mount validation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum MountError {
    #[error("invalid mount identifier")]
    InvalidId,
}
