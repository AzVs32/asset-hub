// These payload types must be `pub` so external callers can match CoreError.
// Their module is crate-private, so callers cannot import the types directly.

use crate::domain::{MountId, VirtualPath};
use thiserror::Error;

#[derive(Debug, PartialEq, Eq, Error)]
pub enum MountServiceError {
    #[error("duplicate mount ID: {0}")]
    DuplicateMountId(MountId),
    #[error("duplicate mount path: {0}")]
    DuplicateMountPath(VirtualPath),
}
