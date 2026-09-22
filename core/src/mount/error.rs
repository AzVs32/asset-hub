use thiserror::Error;

use crate::mount::domain::MountId;
use crate::namespace::domain::VirtualPath;

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceError {
    #[error("duplicate mount ID: {0}")]
    DuplicateMountId(MountId),
    #[error("duplicate mount path: {0}")]
    DuplicateMountPath(VirtualPath),
}
