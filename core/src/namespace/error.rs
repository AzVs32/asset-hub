use thiserror::Error;

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
