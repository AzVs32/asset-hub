use std::error::Error;

use thiserror::Error;

use crate::mount::MountId;
use crate::namespace::VirtualPath;

/// Errors from mount validation and persistence.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MountError {
    #[error("invalid mount identifier")]
    InvalidId,
    #[error("mount ID already exists: {0}")]
    DuplicateId(MountId),
    #[error("mount path already exists: {0}")]
    DuplicatePath(VirtualPath),
    #[error("stored mount contains an invalid {column}")]
    InvalidStoredMount { column: &'static str },
    #[error("mount storage operation failed: {0}")]
    Backend(#[source] Box<dyn Error + Send + Sync>),
}

impl MountError {
    /// Preserves the source of a mount storage failure.
    pub fn backend(error: impl Error + Send + Sync + 'static) -> Self {
        Self::Backend(Box::new(error))
    }
}
