// These payload types must be `pub` so external callers can match CoreError.
// Their module is crate-private, so callers cannot import the types directly.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NamespaceError {
    #[error("virtual path must be absolute")]
    VirtualPathNotAbsolute,
    #[error("virtual path must not contain repeated `/` separators")]
    VirtualPathContainsRepeatedSeparator,
    #[error("virtual path must not end with `/` unless it is the root path")]
    VirtualPathHasTrailingSeparator,
    #[error("virtual path is {length} bytes, exceeding the {max}-byte limit")]
    VirtualPathTooLong { length: usize, max: usize },
    #[error("entry name must not be empty")]
    EntryNameEmpty,
    #[error("entry name must not contain `/`")]
    EntryNameContainsSeparator,
    #[error("entry name must not contain `\\`")]
    EntryNameContainsBackslash,
    #[error("entry name must not contain control characters")]
    EntryNameContainsControlCharacter,
    #[error("entry name must not be `.`")]
    EntryNameIsDot,
    #[error("entry name must not be `..`")]
    EntryNameIsDotDot,
    #[error("entry name is {length} bytes, exceeding the {max}-byte limit")]
    EntryNameTooLong { length: usize, max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DriverKindError {
    #[error("driver kind must not be empty")]
    Empty,
    #[error("driver kind is {length} bytes, exceeding the {max}-byte limit")]
    TooLong { length: usize, max: usize },
    #[error("driver kind contains unsupported character `{character}`")]
    InvalidCharacter { character: char },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid mount identifier")]
pub struct MountIdError;
