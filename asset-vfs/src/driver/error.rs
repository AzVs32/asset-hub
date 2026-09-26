// These payload types must be `pub` so external callers can match VfsError.
// Their module is crate-private, so callers cannot import the types directly.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DriverError {
    #[error(
        "driver kind must be non-empty and contain only lowercase ASCII letters, digits, `.`, `-`, or `_`"
    )]
    InvalidKind,
    #[error("driver kind is {length} bytes, exceeding the {max}-byte limit")]
    KindTooLong { length: usize, max: usize },
}
