// These payload types must be `pub` so external callers can match VfsError.
// Their module is crate-private, so callers cannot import the types directly.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MountError {
    #[error("invalid mount identifier")]
    InvalidId,
}
